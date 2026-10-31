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

use crate::askuser;
use crate::attach;
use crate::store::Attachment;
use crate::config::{self, Config, ModelSettings};
use crate::download;
use crate::editfile;
use crate::gpu::{self, Gpu};
use crate::llama::{self, ModelInfo, Server, ToolCall};
use crate::markdown;
use crate::mcp;
use crate::search;
use crate::store::{self, Chat, Message, Repo};
use crate::tools;
use crate::viewimage;

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

// What the worker is to do with the chat. A turn either answers a message of the
// reader, or goes on with the work from where the talk stands. The second is what
// follows a handoff summary: the summary is written so that the model may carry
// on, and nothing is added to the chat for the model to read, for the handoff
// block that names the summary already tells it to go on.
enum Turn {
    // A message of the reader, with the files beside it.
    Sent(SentMessage),
    // Go on from the talk as it stands.
    Continue,
    // Go on from the talk as it stands, after one message of it was taken out
    // with everything that came after it. The ask is the talk as it stands, and
    // the tools are offered as they are for a message of the reader: the reader
    // asked for the answer again, not for the tail of a handoff summary.
    Retry,
    // Go on with the answer that stands at the end of the talk, after the reader
    // stopped it while the model was still writing. The words the model had put
    // down are the last message of the talk, so the template leaves the mark that
    // opens a saying of the model open, and the model writes on from the last word
    // it had written.
    Resume(String),
}

// What an answer of the model came to, so the worker knows whether the work may
// be gone on with after the room was made.
#[derive(PartialEq, Eq)]
enum Ended {
    // The model came to an end of its own answer.
    Done,
    // Nothing to go on from: the reader stopped the answer, the stream faulted,
    // the room was come to again and the answer gave up, or the turn never began.
    Halted,
}

// Work for the worker thread.
enum Job {
    // The user asked something, with the files of the message beside it.
    Prompt(SentMessage),
    // Another model is wanted, so the running server must go.
    ModelChanged,
    // Settings changed, so a server must not stay with the old values.
    SettingsChanged,
    // The thinking mode or the choice of keeping the thinking moved, so the
    // server must go with its old flags. A choice that did not move never
    // sends this, and the model on the server stays loaded.
    ThinkingChanged,
    // The reader asked for the handoff summary of the open chat to be written
    // again, after one that never came back.
    Handoff,
    // The reader asked for the answer of a message again. The talk was cut back
    // to the message before the ask, so the worker has nothing to look at but the
    // talk as it stands.
    Retry,
    // The reader asked for an answer they stopped to be gone on with. The id is
    // the answer at the end of the talk, and the ask is the talk as it stands.
    Resume(String),
    // The reader added, changed or asked to look again at one server of their
    // own, so the tools it has to offer are read again.
    McpLook(String),
    // Leave the process, after the model is unloaded.
    Shutdown,
}

// The flags a llama-server wears: the ones it was started with, and the ones
// the chat in hand asks for. A running server that wears what the chat asks
// for stands, and the model it loaded stands with it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct ServerWants {
    model_id: String,
    thinking_mode: String,
    preserve: Option<bool>,
    tools: bool,
}

impl ServerWants {
    // Whether a server started with `run` may answer a chat that asks for
    // `wants`. The thinking mode and the choice of keeping the thinking are
    // flags of the server itself, so a change of either sends it away. Tool
    // support only has to cover the chat, and not match it: a server started
    // with it answers a chat that asks for none as plain words, while a server
    // started without it reads no tool call at all. This is what keeps the
    // model loaded between chats, and across the tools badge going on.
    fn covers(run: &ServerWants, wants: &ServerWants) -> bool {
        run.model_id == wants.model_id
            && run.thinking_mode == wants.thinking_mode
            && run.preserve == wants.preserve
            && (run.tools || !wants.tools)
    }
}

// One server of the reader's own, as the page is given it. The words of the code
// of a server of the program itself are kept out of the state, for the state is
// asked for about six times a second and the code may be a long file: the sheet
// of the settings asks for them by a call of its own.
#[derive(Clone, Debug, Default)]
pub struct McpSnap {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub icon: usize,
    // Whether this server stands in the drop-down of the badge.
    pub shown: bool,
    // Whether this server is switched on for the chat in hand.
    pub on: bool,
    // The names of the tools this server last answered with.
    pub tools: Vec<String>,
    // The reason the server could not be looked at, or empty.
    pub fault: String,
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
    // The words of the call so far, as the reader is given them. For a run of
    // the terminal tool it is the command, and for a file tool it is the words
    // the call is making. It is read out of the words of the call, which are
    // often not finished.
    pub command: String,
    // The reason the model gave for the command, as far as it has come. It is
    // the terminal tool alone that ever asked for one.
    pub purpose: String,
    // The one thing the head of the block names: the file a file tool works on.
    // The command of a run names itself, so it stands empty there.
    pub target: String,
    // True when the model has moved on from this call: it is thinking again, or
    // writing words of its own, so nothing more comes of the words being
    // written. The block says so, and the run of the tool stands in the chat in
    // its place when the step of the answer is over.
    pub done: bool,
}

// A question the model has put to the reader, and the wait that stands with it.
// The question stands above the chat box until the reader answers it, the time of
// an ordinary question is gone, or the reader stops the answer; the three roads
// end the wait, and each has its own words for the model.
#[derive(Clone, Debug)]
pub struct Asking {
    // Lets the answer of the reader find the question it belongs to, when a
    // page draws the ask again after the question was asked.
    pub id: String,
    // The question, and the reason the model gives with it.
    pub question: String,
    // The answers the model offered for the reader to choose from.
    pub choices: Vec<String>,
    // True when the model said it cannot go on without an answer.
    pub critical: bool,
    // How long the question is waited for. Nothing at all waits until the reader
    // answers, which is what a question the model cannot go on without asks for.
    pub seconds: u32,
    // The moment the wait began, which is where the time left is measured from.
    began: Instant,
    // What the reader answered. The wait is over when this holds anything.
    answer: Option<String>,
}

impl Asking {
    // The seconds left for the reader to answer, or nothing when the question
    // waits until it is answered. The popup counts these seconds down.
    pub fn left(&self) -> Option<u32> {
        if self.seconds == 0 {
            return None;
        }
        let gone = self.began.elapsed().as_secs() as u32;
        Some(self.seconds.saturating_sub(gone))
    }

    // True when the wait is over: the reader answered, or the time of an ordinary
    // question is gone.
    pub fn ended(&self) -> bool {
        self.answer.is_some() || self.left().map(|left| left == 0).unwrap_or(false)
    }
}

// The words of a call that the reader is given: the command of a terminal run,
// or the one thing another tool names. The head of the block of a run and the
// block of a call being written both come from here, so the two cannot say
// different things about one call. The reason the model gave rides along for the
// terminal tool, which is the only tool that ever asked for one.
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
        // A file being offered to the reader is named by the file itself.
        download::NAME => {
            let one = download::partial_words(raw);
            (one.clone(), String::new(), one)
        }
        // A question of the reader is named by the question.
        askuser::NAME => {
            let one = askuser::partial_words(raw);
            (one.clone(), String::new(), one)
        }
        // A look about the web is named by the words to look for, or by the
        // address that is to be read.
        search::NAME => (search::partial_words(raw), String::new(), String::new()),
        viewimage::NAME => (viewimage::partial_words(raw), String::new(), String::new()),
        // A tool of a server of the reader's own is named by the tool itself,
        // and the parts the model is writing are the words of the block.
        name if mcp::is_tool(name) => (
            mcp::shown_tool(name),
            String::new(),
            mcp::partial_words(raw),
        ),
        // A name the program does not offer, and a name that came in pieces: the
        // words of a file are the best guess of what the model is naming.
        _ => (editfile::partial_words(raw), String::new(), String::new()),
    }
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

// One summarisation of a chat, as the window needs it. A summarisation is kept
// in the chat as a message of its own, so it stands where the talk was cut, and
// the window draws it as the heading of the summarisation and as a node on the
// timeline at the right of the chat.
#[derive(Clone, Debug, Default)]
pub struct Handoff {
    // The id of the message that holds the summarisation.
    pub id: String,
    // How many times this chat has been summarised up to and including this
    // one. The heading says "Summarised #2" for the second of them.
    pub number: usize,
    // What became of the summary: "writing" while the model is at it, "done"
    // when the words came, and "owed" when they never came and the reader can
    // start the summary again.
    pub state: String,
    // When the summarisation stood, in seconds since 1970.
    pub when: i64,
    // The room the talk took before it was summarised, in tokens.
    pub before: u64,
    // The room the handoff summary itself takes, in tokens.
    pub after: u64,
    // A little of the handoff summary, for the card of a node. Empty while the
    // summary was never written.
    pub snippet: String,
    // What went wrong, when the summary was never written.
    pub why: String,
}

// The summarisations of one chat, in the order they were made. `writing` says
// that the program is at a summary right now, which is the one case in which an
// empty summarisation is not yet a fault.
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
        });
    }
    out
}

// True when the words are not a saying of the model at all, but a call of a tool
// that was cut short: the mark of a call at the front, and no closing mark,
// because the room gave out in the middle of it. Such words are not a summary,
// and the next model would read them as one.
fn call_of_a_tool(text: &str) -> bool {
    let open = String::from("<") + "|";
    let close = String::from("|") + ">";
    let Some(at) = text.find(open.as_str()) else {
        return false;
    };
    let rest = &text[at + open.len()..];
    !rest.contains(close.as_str()) && rest.trim_start().starts_with("im_start|tool")
}

// A little of a handoff summary, for the card of a node on the timeline. The
// marks of the writing are taken off, for a node has a small room to be read in.
fn summary_snippet(text: &str) -> String {
    let flat = text
        .replace("```", " ")
        .replace(|c| matches!(c, '#' | '*' | '`' | '>' | '~'), "")
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

// True when a line of the answer of the model only hands the summary over,
// such as "Here is the handoff summary:". Those words are not part of the
// summary, and they cost room on every turn that comes after it.
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

// True when a line of the answer only offers a thing of the model's own, such
// as "Let me know if you need more detail.". As above, it is not part of the
// summary and it is not wanted at the end of it.
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

// The handoff summary the model gave, cleaned of what the model wrapped it in.
// A model asked for a summary sometimes puts a line of its own before it, or
// the whole of it inside a fence, or an offer after it. None of those words are
// part of the summary: they are the envelope of it, and they are sent again on
// every turn after the compaction. The marks of the writing are left where they
// stand, for the summary is put on the page again by the next model.
pub fn clean_summary(text: &str) -> String {
    // The marks of a turn that are not words. A model sometimes repeats one of
    // them inside its answer, and such a mark has no business going back.
    let bare: String = text
        .chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect();
    let mut lines: Vec<&str> = bare.lines().map(|line| line.trim_end()).collect();
    // A fence about the whole of it is the wrapper of the summary: the prompt
    // asks for no styling, so a fence at either end is not the summary itself.
    // The wrapper is taken off again after the two loops below, for a model that
    // hands the summary over with a line of its own tends to shut the fence under
    // that line rather than under the last word of the summary.
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
    // A model that answers the ask with a call of a tool, rather than with the
    // summary, has written nothing that the next model can use. The words of a
    // call that the room cut short are thrown away here, so the ask is made again
    // rather than a call of a tool standing in the place of a summary.
    if call_of_a_tool(text) {
        return String::new();
    }
    // Two blank lines one behind the other say nothing, and the next model pays
    // for every one of them.
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

// The words that go round a handoff summary when it is sent to the model again.
// The model has to know that the talk it is reading was made shorter, that the
// text it is given was written by a model at the point of the compaction, and
// that it is there to go on with the work and not to begin it again.
// The first words of the block that goes round a handoff summary. They are
// looked for whole, and only at the front of a word that goes out, so no other
// saying is taken for one.
pub const HANDOFF_FIRST: &str = "[The session was compacted";

pub fn handoff_block(summary: &str) -> String {
    format!(
        "{} The messages between the first user message and this point were removed, so that the conversation could continue inside the context window of the model. The text below is a handoff summary written by an AI at that point, so that the session can continue. Read it as the state the session was in, and continue the work from it. Do not greet the user, do not repeat this summary, and do not start the work again.]\n\n{}",
        HANDOFF_FIRST, summary
    )
}

// The words the program puts in the place of the reader when a turn of the chat
// only goes on, and the talk it is given ends on a call the server has to answer.
// Such a turn carries no message of the reader of its own, so the server is asked
// to pay the debt of that call instead of going on with the work: it writes a
// call, the words of it are thrown away with the ask that made it, and no answer
// of the model comes at all. These words go out after the talk as a message of
// the reader, so the ask ends on a question. They are never written into the chat,
// for the reader never wrote them and has nothing to read where they stand.
pub const CONTINUE_PROMPT: &str = "[Go on with the work from the point it reached above. Do not greet me and do not begin the work again. Read back to me what the last run printed, answer in words when no tool is wanted, and call a tool only when the work needs one. A tool named above is off unless it stands in the list you are given now.]";

// The line that stands under a tool call which was stopped where the model was
// still writing it. The head of the block says what the tool was for; this says
// what became of it, in words the reader does not have to be a model to follow.
pub const CANCELLED_WORDS: &str = "The call was cancelled. The conversation reached the summarisation limit while the call was being written, so the program stopped the writing and the tool never ran";

// The same line, for a call the reader stopped with their own hand. Nothing of
// the room stands behind it: the reader pressed stop, and the tool never ran.
pub const STOPPED_WORDS: &str = "The call was cancelled: you stopped the answer while the call was being written, so the writing was stopped and the tool never ran";

// The most of the words of a cut-off tool call that go back to the model. The
// words of such a call are seldom short: a model writing a file out has the
// whole of the file in them, and the room the summarisation just made is eaten
// up by sending all of that again. The head of a call names what it was for --
// the file, the command, the first of the words -- which is the part worth the
// room.
pub const GREATEST_CUT_ARGS: usize = 4000;

// The least of the words of a cut-off call that are kept, whatever the room. A
// call is kept for what its head names, and fewer words than this would not even
// name the file the model was writing into.
// The head of a call is what names the work it was, and a head of fewer words
// than this would not even name the file the model was writing into.
pub const LEAST_CUT_ARGS: usize = 120;

// How much of the words of a cut-off call may go back to the model. The room the
// summarisation made is spent on the words of the call as much as on anything
// else, and a telling that filled the room again would have the model stopped a
// second time before it had a word to give -- which is what the limit is looked
// at for. A tenth of the room is therefore held back over it, for the answer the
// model is to write, and what is left over is turned into characters at the four
// characters to a token the estimate of a talk goes by.
pub fn cut_bound(used: u64, total: u64) -> usize {
    if total == 0 {
        // Nothing is known of the room, so the most a telling is ever given.
        return GREATEST_CUT_ARGS;
    }
    let left = total.saturating_sub(used + total / 10);
    (left as usize * 4).clamp(LEAST_CUT_ARGS, GREATEST_CUT_ARGS)
}

// The first `wanted` characters of a text, counted as characters and not as
// bytes, for the words of a call hold whatever the model was writing.
fn head_chars(text: &str, wanted: usize) -> String {
    text.chars().take(wanted).collect()
}

// The words of a tool call the room gave out under, given back to the model as
// the words of the turn that was cut short. Three things have to come across:
// that the call was cancelled and never ran, that the words of it are only as
// much as the model had written when it was stopped, and that the room has since
// been made, so the work may go on. The whole call is asked for again, for half
// a call is of no use to anyone -- and a call bigger than the room is asked for
// in pieces, for the same room will give out under it again.
//
// Whether the words of the call stop in the middle is told by the words
// themselves: a call that shut all its marks is whole as far as anyone can tell,
// and a call with a string left open in it is not, which is what the model has to
// hear either way.
pub fn cut_words(name: &str, args: &str) -> String {
    format!(
        "[A tool call was cancelled. The conversation reached the summarisation limit while the model was still writing the call, so the program stopped the writing and the tool never ran: nothing of what the call asked for was done. Space has since been made by a handoff summary, and the conversation continues. The call as it was when the writing stopped is given below. If the work still needs this, write the call again from the start, in parts small enough to fit in the context the model has.\n{}\n{}]",
        named_call(name),
        cut_call_text(name, args)
    )
}

// The words of a tool call the reader stopped. Two things differ from a call the
// room gave out under: nobody made room after it, and the answer is over, so
// nothing is asked of the model now. The call is still told back, for the model
// is to know what it was put to when the reader asks it for the work again.
pub fn stopped_words(name: &str, args: &str) -> String {
    format!(
        "[A tool call was cancelled. The reader stopped the answer while the model was still writing the call, so the program stopped the writing and the tool never ran: nothing of what the call asked for was done. The answer is over, and the context window was not touched. The call as it was when the writing stopped is given below. If this work is taken up again, write the call again from the start, in parts small enough to fit in the context the model has.\n{}\nText of the call written before it was stopped:\n{}\n{}]",
        named_call(name),
        args,
        words_as_they_stood(args)
    )
}

// The words of a call that was cut short, as the model is told them. The head
// of the call names the work it was, so it stands first, and the words of it
// are kept to the head: a whole file in the arguments of a cut call is the work
// the summary already stands for, and sending it again whether the call is whole
// or cut, and the head is the part worth the room.
// The reader is never shown the telling, so the block they see keeps the whole
// of the words it had, and only the words that go out are kept to the head.
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

// The tool the call was pointed at, or the words that say its name was never
// written.
fn named_call(name: &str) -> String {
    if name.trim().is_empty() {
        String::from("the model had not written the name of the tool yet")
    } else {
        format!("tool: {}", name)
    }
}

// What the words of a stopped call say about themselves. Whether they stop in the
// middle is told by the words themselves: a call that shut all its marks is whole
// as far as anyone can tell, and a call with a string left open in it is not,
// which is what the model has to hear either way.
fn words_as_they_stood(args: &str) -> String {
    if serde_json::from_str::<Value>(args).is_ok() {
        String::from("(the text of the call is kept as the model wrote it. The writing was stopped, and the tool never ran)")
    } else {
        String::from("(the text of the call ends in the middle: the model had written no further when the writing stopped, so what it intended after this point is not known)")
    }
}

// The words the Chatting settings can put before the model, so it knows the day
// and the clock time it works on. They stand once for a talk, in a message of
// the role `system` at its head, and never beside a message of the reader.
// Nothing of the line is shown among the words of the chat: it is a saying of
// the program on the way to the model, as the handoff block is.
pub const DATE_HEAD: &str = "[Program note: ";

// One line of the day and the clock time, written from the moment it is given.
fn date_line(secs: i64) -> String {
    let named = if secs > 0 { secs } else { store::now_secs() };
    format!(
        "{}the date and the time on this machine are {}, in the local time zone of this \
         machine. This is the latest the machine has. Answer from it when the reader asks for \
         today, for the day of the week or for the hour; do not say the date is unknown. The \
         reader did not write this line and is not shown it, so do not speak of it and do not \
         reply to it: it is a note, not a question, and there is nothing in it to answer.]",
        DATE_HEAD,
        store::format_date_clock(named)
    )
}

// The moment the line of the day is written from: the moment the chat was begun.
// A chat with no moment of its own is named from this moment, and keeps it from
// then on, as a chat begun now is.
fn began_of(chat: &store::Chat) -> i64 {
    if chat.meta.created > 0 {
        chat.meta.created
    } else {
        store::now_secs()
    }
}

// The message of the role `system` that carries the day and the clock time, or
// nothing at all when the switch of the Chatting settings stands off. The line
// goes at the head of the talk once, so no message of the reader carries it and
// no message of the reader carries it twice.
//
// `began` is the moment the chat was begun, and the line is written from that
// moment alone. The head of the talk must not move while the talk runs, for the
// server keeps the work it did on the head of a prompt only while the head
// stands word for word as it stood: a line written from the clock of this moment
// made a different head every minute, and every turn after it - the answer of a
// tool above all, which comes seconds after the call was made - was read from
// the start again, whatever the switch of prompt caching said. A run of a model
// on this box measured the difference: 655 tokens read over in 26.9 seconds with
// a head that had moved by a minute, and 4 tokens read over in 0.34 seconds with
// a head that had not moved at all.
pub fn date_message(cfg: &Config, began: i64) -> Option<Value> {
    if !cfg.chat_date_new_chat {
        return None;
    }
    Some(json!({
        "role": "system",
        "content": date_line(began),
    }))
}

// Where the newest handoff summary that was really written stands among the
// messages of a chat. None means the chat was never summarised.
fn last_summary_place(chat: &Chat) -> Option<usize> {
    chat.messages.iter().rposition(|message| {
        message.role == "Summary" && !message.body.trim().is_empty()
    })
}

// True when the talk that goes out to the server ends on a call the server has
// to answer: a call of a tool, the receipt of one, or the telling of a call that
// was stopped while the model wrote it. A server set to read calls
// out of an answer is put to work behind such a call, and the words of the turn
// that holds it are never given back. The two asks that are not an answer of the
// model -- the ask for a handoff summary, and the ask that puts the model back to
// work behind one -- therefore have to name the words of that call themselves:
// `handoff_block` says not to answer what stands before it, and a call that begs
// an answer is the one thing a model cannot read past. It writes the call instead
// of going on with the work, and the call of a summary ask is thrown away, so
// nothing at all comes of the ask and the chat stands still under its heading.
fn ends_on_a_call(history: &Value) -> bool {
    match history.as_array().and_then(|items| items.last()) {
        Some(last) => {
            last.get("tool_calls").is_some()
                || last.get("role").and_then(|r| r.as_str()) == Some("tool")
                // The telling of a call the room gave out under is a call as well:
                // it stands in the place of the turn that was cut short, and the
                // model is asked for that call again. A word of the program goes
                // out after it, so the ask ends on a question and what comes back
                // is an answer and not a thing thrown away.
                || last
                    .get("content")
                    .and_then(|c| c.as_str())
                    .is_some_and(|said| said.starts_with(CUT_TOLD_FIRST))
        }
        None => false,
    }
}

// The first words of `cut_words`. They are looked for whole, and only at the
// front of the words that go out, so no other saying is taken for one.
pub const CUT_TOLD_FIRST: &str = "[A tool call was cancelled";

// The talk of an ask that only goes on, after a handoff summary or after a run of
// a tool. The mark of the summarisation carries its handoff block in the history
// already, so an ask of this shape needs nothing more than the talk as it stands
// -- unless the talk ends on a call or on the receipt of one. Then a word of the
// program goes out after it as a message of the reader, so the talk ends on a
// question and the server is asked for an answer rather than for the call it would
// else write behind the receipt. Nothing is added in any other case: the block of
// a summarisation is in the talk already, and a second copy of it would cost the
// room the summarisation was made to win.
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

// True when the newest thing of a chat is a summarisation, which is to say
// there is nothing new to write a summary of.
fn ends_on_summary(chat: &Chat) -> bool {
    chat.messages
        .iter()
        .rev()
        .find(|message| message.role != "Note")
        .map(|message| message.role == "Summary" && !message.body.trim().is_empty())
        .unwrap_or(false)
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
    // True while the program is writing a handoff summary, whose words are never
    // shown to the reader.
    pub summarising: bool,
    pub busy: bool,
    pub code_enabled: bool,
    // Which of the tools of the program the model may reach in the open chat.
    // The badge of the tools is the switch over all of them; these are the lines
    // of its drop-down.
    pub tool_switches: store::ToolSwitches,
    // The switch of the web search of the open chat.
    pub search_enabled: bool,
    pub sandbox_enabled: bool,
    pub sandbox_available: bool,
    // What a command of the open chat may reach beyond its own folder: the
    // network, an install of a Python package, and the commands that fetch a page
    // of the web. All three stand off, and the menu of the sandbox badge opens
    // them one by one.
    pub sandbox_access: tools::Access,
    // The servers of the reader's own, for the drop-down of the badge and the
    // MCP tab of the settings.
    pub mcp: Vec<McpSnap>,
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
    // Whether a thinking panel grows with its words instead of stopping at a height.
    pub chat_grow_thinking: bool,
    // Whether a tool block called while thinking stands inside the thinking panel.
    pub chat_tools_in_thinking: bool,
    // Whether the day and the hour go before the model once, at the start of
    // the talk, in a message of the role `system`.
    pub chat_date_new_chat: bool,
    // Whether a block of code takes the colours of the kinds of word in it.
    pub chat_highlight: bool,
    // Whether the marks of boldness are read as boldness in the words of the
    // chat, or as part of the words themselves.
    pub chat_bold_markdown: bool,
    // How many times an answer is asked over when the model came to its own end
    // without a word, a call or a fault.
    pub chat_silent_retries: i64,
    // Whether the model may mark the chapters of a chat, and whether the
    // chapter line stands beside it.
    pub chat_chapters: bool,
    // Whether the talk that the model has worked its way through is kept in a
    // file of the program, so that a chat coming back is not read through
    // again. The sheet of the settings reads it, and the page has nothing of
    // its own to do with it: the keeping is all behind the ask.
    pub chat_context_files: bool,
    // The number of characters where a paste of words into the chat box turns
    // into a file of its own.
    pub chat_paste_limit: u32,
    // How long the model waits for an answer to a question it put to the reader,
    // for the two kinds of question. The second is nought by default, which means
    // the question is waited for until it is answered.
    pub ask_seconds: u32,
    pub ask_critical_seconds: u32,
    // How wide the chat may be, in pixels. The page keeps it within 80 per cent
    // of the window, and out of the room the two lines and the arrows need.
    pub chat_width: u32,
    pub models: Vec<ModelInfo>,
    pub model_id: String,
    pub repositories: Vec<Repo>,
    pub chat_id: String,
    // True when no chat is open. The window then shows the chat box in the
    // middle of the screen, with the program mark and a line for this hour. The
    // side bar and this one screen are what the reader goes between: a new chat
    // is this screen, and the first message of it grows the chat.
    pub home: bool,
    // The line above the chat box while no chat is open.
    pub greeting: String,
    pub messages: Vec<Message>,
    // The chapters of the open chat, in the order the model marked them.
    pub chapters: Vec<Chapter>,
    // The summarisations of the open chat, oldest first, for the heading of the
    // summarisation and the timeline of them at the right of the chat.
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
    // The two speeds of the work, in tokens of the talk to the second: how fast
    // the words that were sent are being worked through, and how fast the model
    // is writing. The server is asked for both, and each stands at nought until
    // it says something of speed.
    pub prompt_rate: f64,
    pub answer_rate: f64,
    // The calls of a tool the model is writing right now, in the order it
    // writes them. A block of a command being written is made of these.
    pub live_tools: Vec<LiveTool>,
    // The question the model has put to the reader and is waiting on, when it has
    // put one. The popup of the ask stands above the chat box.
    pub ask: Option<Asking>,
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
    // Repository the side bar has open, and the one a new chat belongs to.
    pub scope_repo: String,
    // The chats of the place that is open, newest first. The side bar reads the
    // chats of every place when it is asked for them; this list is the place the
    // open chat stands in.
    pub chats: Vec<Chat>,
    pub current: Option<String>,
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
    // The two speeds of the work, as the page shows them: the words sent to the
    // server, and the words the model is writing. Both are nought until the
    // server says something of speed, or the clocks of the program have had
    // their say.
    pub prompt_rate: f64,
    pub answer_rate: f64,
    // The moment the words of an answer began to come. The program measures the
    // speed of the writing itself when the server says nothing of speed, and it
    // cannot do that without a beginning to measure from.
    rate_start: Option<Instant>,
    // The calls of a tool the model is writing right now, in the order the
    // model writes them. The reader sees a command as it is written.
    pub live_tools: Vec<LiveTool>,
    // The question the model has put to the reader and is waiting on, when it has
    // put one. The ask stands above the chat box until it is answered, given up,
    // or stopped with the answer.
    pub asking: Option<Asking>,
    // The moments the thinking of the live answer began and last stopped. The
    // two together are the length of it.
    thought_start: Option<Instant>,
    thought_end: Option<Instant>,
    // The moment the meter of the context last moved. The meter is paced, so it
    // does not write a new number on every piece of the answer.
    ctx_at: Option<Instant>,
    pub generating: bool,
    // True while the program is writing a handoff summary. The words of a
    // summary are never shown, so the window draws no live answer while this
    // stands; the heading of the summarisation and the status line say that the
    // program is at work.
    pub summarising: bool,
    // The model is loading, so the window shows a spinner.
    pub busy: bool,
    pub status: String,
    pub ctx_total: u64,
    pub ctx_used: u64,
    // The first number of the talk that the server gives for a new ask is taken
    // where it stands, and after that the meter only climbs. The number left over
    // from the ask before may count words that were never kept in the chat -- an
    // answer the reader stopped leaves the words the model had written out of the
    // talk again -- and a meter that only climbs would stand still for the whole
    // of the answer that followed, for ever after.
    pub ctx_fresh: bool,
    // The moment last handed to a chat that was opened. Two chats opened inside
    // one second are told apart by it, so the one opened last stands at the head
    // of its group none the less.
    pub last_opened: i64,
    pub sandbox_available: bool,
    pub sandbox_note: String,
    // What the chat template of the current model can do with thinking.
    pub thinking: markdown::ThinkingOptions,
    pub rev: u64,
    pub structure_rev: u64,
    // The chat settings a new chat starts with. They matter while no chat is
    // open, where a badge has no chat to change.
    pub new_code: bool,
    // The lines of the drop-down of the badge of the tools that the next chat
    // opens with, as the choice of them in this chat leaves them.
    pub new_tools: store::ToolSwitches,
    pub new_search: bool,
    pub new_sandbox: bool,
    // What a command of the next chat may reach. A new chat takes the choices of
    // the chat that stood at the front, as it takes the other badges.
    pub new_sandbox_access: tools::Access,
    pub new_thinking: String,
    pub new_preserve: bool,
    // The MCP servers the next chat starts with, by their id. A new chat takes
    // them from the chat that stood at the front, as it takes the badges.
    pub new_mcp: Vec<String>,
    // What each server last answered when it was asked what it has to offer,
    // kept by its id. The tools of a server are read once and kept, so a chat
    // does not open a server again on every answer, and the sheet of the
    // settings has something to say about a server beside its name.
    pub mcp_tools: std::collections::BTreeMap<String, Vec<mcp::Tool>>,
    // The reason a server could not be looked at, kept by its id. A server that
    // answers has no entry here.
    pub mcp_fault: std::collections::BTreeMap<String, String>,
    // Which slot of the running server each chat had its talk read into. The
    // server understands a talk in one slot and nowhere else, so a chat that
    // comes back wants the slot it stood in, and the ask of it has to name the
    // slot to the server. A chat with no entry here has nothing to go back to,
    // which is how things stand when the keeping of the talks is switched off.
    pub context_slot: std::collections::BTreeMap<String, usize>,
    // Which chat stands in each slot of the running server, the other side of
    // the list above. A slot that another chat is to take has to be written out
    // to the file of the chat that stands in it first, for the talk of that
    // chat stands nowhere else in the world once the slot is gone over.
    pub context_holds: std::collections::BTreeMap<usize, String>,
}

// Whether the talk that goes out ends on a saying of the model with words of its
// own and no call on it. An answer that was gone on from must stand open at the
// end of the talk, for the template puts the words of the last saying of the
// model under the mark that opens an answer and leaves that mark open. A talk
// that ends on a call, on a receipt, or on a handoff summary is not open, and
// nothing is asked of the model there.
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
        // A message of parts is a message of words and pictures.
        Some(Value::Array(_)) => true,
        _ => false,
    }
}

// Take the mark of an answer that never came to its end off every message of
// the talk. The mark stands on one answer at most, and that answer is the newest
// one the reader stopped.
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

    // Say that the server knows nothing of any talk any more. The talks of the
    // chats stood in the slots of that one server: a server started again has
    // slots of its own, and nothing of an older server is understood by it. The
    // talks themselves are safe -- they were written out before the server was
    // stopped, and the words of a chat stand in its file of chats as ever.
    pub fn context_lost(&mut self) {
        self.context_slot.clear();
        self.context_holds.clear();
    }

    // The thinking mode of the chat in hand. With no chat open, the mode the
    // badge holds for the next chat is used, so the badge and the flags of the
    // server cannot drift apart.
    fn thinking_of_open(&self) -> String {
        self.chat()
            .map(|c| c.meta.thinking_mode.clone())
            .unwrap_or_else(|| self.new_thinking.clone())
    }

    // What a server has to wear for the chat in hand: the thinking mode, the
    // choice of keeping the thinking, and whether it must be able to read tool
    // calls. The running server is compared against these, so a chat that
    // stands where the one before it stood asks for the same server the one
    // before it ran on, and the model on it stays loaded.
    fn server_wants(&self) -> ServerWants {
        ServerWants {
            model_id: self.current_model_id(),
            thinking_mode: self.thinking_of_open(),
            preserve: self.thinking.has_preserve.then(|| self.preserve_of_open()),
            tools: self.tools_for_open(),
        }
    }

    // The flags the running server stands on, in the same shape as the ones a
    // chat asks for, so the two can be weighed against each other.
    fn server_wears(running: &Server) -> ServerWants {
        ServerWants {
            model_id: running.model_id.clone(),
            thinking_mode: running.thinking_mode.clone(),
            preserve: running.preserve_thinking,
            tools: running.tools_on,
        }
    }

    // Whether the chat in hand wants a server that can read tool calls. Any
    // tool is enough: the chapters and the web search are wanted even in a chat
    // that never runs a command. With no chat open, the badges decide.
    fn tools_for_open(&self) -> bool {
        self.chat()
            .map(|c| c.meta.code_enabled || c.meta.search_enabled)
            .unwrap_or(self.new_code || self.new_search)
            || self.cfg.chat_chapters
            || !self.mcp_of_open().is_empty()
    }

    // Every server of the reader's own, in the order the reader put them, with
    // what the program last knew of each.
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

    // The servers of the reader's own that stand switched on for the chat in
    // hand. Only servers that still stand in the settings are named, so a chat
    // that outlived one of them asks for nothing that is gone. With no chat
    // open, the list the badge holds for the next chat is used.
    pub fn mcp_of_open(&self) -> Vec<String> {
        let held = self
            .chat()
            .map(|c| c.meta.mcp.clone())
            .unwrap_or_else(|| self.new_mcp.clone());
        held.into_iter()
            .filter(|id| self.cfg.mcp_servers.iter().any(|s| &s.id == id))
            .collect()
    }

    // The servers that stand switched on for the chat in hand and whose tools
    // were never read. They are looked at out of the state lock, for a server
    // that will not answer is waited for, and the window must be free to draw
    // while it is waited for.
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

    // The tools of the servers that stand switched on, as the model is sent
    // them. A server with nothing to offer, or with nothing read of it, gives
    // no tool, and the chat goes on without it.
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

    // Open the chat that stands at this place in the list, and put it at the
    // head of its group in the side bar. The moment it was opened is kept with
    // the chat, which is what the order of a group is read from.
    fn mark_opened(&mut self, place: usize) {
        // The moment of the open never stands below the last one handed out, so
        // two chats opened within one second keep the order the reader opened
        // them in, and the chat that was opened last is the first in its group.
        let now = store::now_secs();
        let stamp = if now > self.last_opened { now } else { self.last_opened + 1 };
        self.last_opened = stamp;
        self.chats[place].meta.opened = stamp;
        let snapshot = self.chats[place].clone();
        self.ctx_used = snapshot.meta.context_used;
        self.current = Some(snapshot.meta.id.clone());
        self.store.save_chat(&snapshot);
        self.set_status("Ready");
        self.touch_structure();
    }

    // Change the record of one chat, of this place or of another. The place in
    // hand is looked in first, for the chat that stands open lives there in its
    // newest shape; after it the chat is read from the disk. `None` says that no
    // place holds such a chat.
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
            self.store.save_chat(&snapshot);
            self.touch_structure();
            return Some(());
        }
        let mut chat = self.store.load_chat(id)?;
        change(&mut chat);
        self.store.save_chat(&chat);
        self.touch_structure();
        Some(())
    }

    // Every repository with the chats that stand in it, the chats that belong to
    // no repository, and the chats the reader put away. A chat that was put away
    // stands in neither of the first two. The chats of a group stand newest
    // opened first, so the chat the reader was last in is the first of its group.
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
        // The chats that were put away stand newest first, for the reader is
        // most likely to want the one they put away last.
        archived.sort_by_key(|c| std::cmp::Reverse(c.meta.updated));
        Lists {
            repositories,
            loose,
            archived,
        }
    }

    // Write the open chat to disk. Cloning first keeps the borrow short.
    //
    // A chat that holds nothing is not written at all. It stands in the memory
    // until the first message makes it worth keeping, so a new chat that the
    // reader started and left alone leaves no file behind and never shows in
    // the side bar.
    fn persist(&mut self) {
        let Some(chat) = self.chat() else { return };
        if chat.messages.is_empty() {
            return;
        }
        // The first message makes the chat a file, and a file makes a row in the
        // side bar, so the lists of the windows are made anew.
        let first = chat.messages.len() == 1;
        let snapshot = chat.clone();
        if first {
            self.touch_structure();
        }
        self.store.save_chat(&snapshot);
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

    // The words a resumed answer adds to the answer the reader stopped. What the
    // model wrote before the stop, and what it wrote on from those words, is one
    // answer of the model and not two, so the new words go onto the message that
    // stands at the end of the talk. False means the message is no longer the last
    // one of the talk, and the words have to stand on their own.
    fn append_resumed(&mut self, id: &str, body: &str, reasoning: &str, reasoning_ms: u64) -> bool {
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
        true
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

    // The answer the reader stopped is the one answer the page may go on with.
    // The mark therefore goes on the newest saying of the model, and off every
    // older one: two answers under a mark of going on read as a fault, and an
    // answer the reader has written past is not the one they meant.
    fn mark_last_answer_stopped(&mut self) {
        let Some(chat) = self.chat_mut() else { return };
        clear_stopped_marks(chat);
        if let Some(last) = chat.messages.last_mut() {
            if last.role == "Assistant" {
                last.stopped = true;
            }
        }
        self.persist();
        self.touch();
    }

    // The mark of an answer that never came to its end is taken off as careful as
    // it is put on: an answer that came to its end, and a new turn of the chat,
    // both leave nothing to go on with.
    fn clear_stopped_marks(&mut self) {
        let Some(chat) = self.chat_mut() else { return };
        if !chat.messages.iter().any(|one| one.stopped) {
            return;
        }
        clear_stopped_marks(chat);
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
            "The chapter needs a name, so it was not marked"
        } else {
            "The chapter was marked"
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

    // Put the mark of a summarisation in the chat, before the handoff summary
    // is asked for. The mark keeps its place whether the words come or not: the
    // reader sees where the talk was cut, and a summary the server never
    // finished is a mark with nothing in it, which the reader can start again.
    //
    // A mark with nothing in it is started again in the place it stands, and is
    // not copied. The reader is told to write that summarisation again, so a
    // second heading with a new number would say the chat was summarised twice
    // when it was not: the retried mark keeps its number and its place, and only
    // its words and its hour are put again.
    fn begin_summary(&mut self) -> String {
        if let Some(chat) = self.chat_mut() {
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
                self.persist();
                self.touch();
                return again;
            }
        }
        let id = store::new_id();
        if let Some(chat) = self.chat_mut() {
            chat.messages.push(Message {
                id: id.clone(),
                role: "Summary".to_string(),
                tool: String::from("The handoff summary is being written ..."),
                when: store::now_secs(),
                ..Default::default()
            });
        }
        self.persist();
        self.touch();
        id
    }

    // Write the handoff summary into its mark. The words the model gave are
    // cleaned first, and `why` holds the line the window shows under the number
    // of the summarisation: a short word of what happened, or of what went
    // wrong. The two numbers of the room stand with the mark, so the reader can
    // see what the compaction cost.
    //
    // A summarisation stands where the talk was cut, so whatever of the talk
    // stands after the mark is carried to the front of it. The mark goes into
    // the chat at the end of it before the ask, for a server that goes away
    // in the middle of a summary must leave a sign of it. But the cut was
    // made before the ask went out: the words and the tool blocks of the
    // answer that was cut stand before the mark, and the summary stands for
    // them. Left after it, they would read as the words that came after the
    // compaction, and the model would read the summary and the words of a
    // turn it was told to carry on from, with the thinking of that turn
    // standing in the middle. The mark is the cut of the talk, and the cut
    // stands before the words it cuts off.
    fn finish_summary(&mut self, id: &str, summary: &str, before: u64, after: u64, why: &str) {
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
            let moved: Vec<Message> = chat.messages.drain(place + 1..).collect();
            if let Some(mark) = chat.messages.get_mut(place) {
                mark.body = summary.to_string();
                mark.context_before = before;
                mark.context_after = after;
                mark.tool = why.to_string();
            }
            chat.messages.extend(moved);
        }
        self.persist();
        self.touch();
    }

    // True when a handoff summary of the open chat was asked for and never came
    // back. The chat stands at that point until the reader starts the summary
    // again, for the talk past the room of the model cannot be sent as it is.
    fn handoff_owed(&self) -> bool {
        self.chat().map(|chat| chat.handoff_owed()).unwrap_or(false)
    }

    // The summarisations of the open chat, oldest first.
    fn handoffs(&self) -> Vec<Handoff> {
        self.chat()
            .map(|chat| handoff_marks(chat, self.generating))
            .unwrap_or_default()
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
                tool: String::from("Running ..."),
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
        self.finish_tool_with(id, output, ok, Vec::new());
    }

    // Put a question to the reader and begin the wait. The words of the question,
    // the answers the model offered, and the wait that stands with them all come
    // from the ask itself and the settings; the popup above the chat box is made
    // of what the state carries.
    fn begin_asking(&mut self, asked: &askuser::Question) -> (String, u32) {
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
            began: Instant::now(),
            answer: None,
        };
        let id = one.id.clone();
        let seconds = one.seconds;
        self.asking = Some(one);
        self.touch();
        (id, seconds)
    }

    // The answer of the reader, when the question the page was drawing is the one
    // that stands. A question that was answered already, or that the run has
    // given over, is answered no more.
    fn answer_asking(&mut self, id: &str, answer: &str) -> bool {
        let Some(one) = self.asking.as_mut() else {
            return false;
        };
        if one.id != id || one.answer.is_some() {
            return false;
        }
        one.answer = Some(answer.to_string());
        self.touch();
        true
    }

    // What the reader answered the question of this id, when they have. The run of
    // the tool asks this until the wait comes to an end.
    fn asking_answer_of(&self, id: &str) -> Option<String> {
        self.asking
            .iter()
            .find(|one| one.id == id)
            .and_then(|one| one.answer.clone())
    }

    // Take the ask away, for the question came to its end and the popup over the
    // chat box has nothing left to say. A question of another run is left alone.
    fn clear_asking(&mut self, id: &str) {
        if self
            .asking
            .as_ref()
            .map(|one| one.id == id)
            .unwrap_or(false)
        {
            self.asking = None;
            self.touch();
        }
    }

    // The same, with the files a run of the download tool offered to the reader.
    // The offer rides on the message of the run, so the blocks can be drawn again
    // when the chat is opened another day; the blocks themselves stand at the end
    // of the answer, and only once the model has stopped writing. See
    // `gifts_till` for the offer of a turn.
    fn finish_gifts(&mut self, id: &str, output: &str, ok: bool, gifts: Vec<download::Gift>) {
        if let Some(chat) = self.chat_mut() {
            if let Some(message) = chat.messages.iter_mut().find(|m| m.id == id) {
                message.tool = output.to_string();
                message.failed = !ok;
                message.downloads = gifts;
            }
        }
        self.persist();
        self.touch();
    }

    // The same, with the records of the pictures the run opened hanging on the
    // message as well. A tool that hands a picture to the model keeps the bytes
    // of it out of the words the reader is given, so the picture stands where the
    // pictures of a chat stand: in the folder of the records, named by the
    // message that opened it.
    fn finish_tool_with(
        &mut self,
        id: &str,
        output: &str,
        ok: bool,
        pictures: Vec<Attachment>,
    ) {
        if let Some(chat) = self.chat_mut() {
            if let Some(message) = chat.messages.iter_mut().find(|m| m.id == id) {
                message.tool = output.to_string();
                message.failed = !ok;
                message.attachments = pictures;
            }
        }
        self.persist();
        self.touch();
    }

    // Write down a tool call that the room gave out under. The call stands in the
    // chat as the thing it was: writing that was stopped, and a tool that never
    // ran. The words of it are the words the reader watched being written, and the
    // whole of them rides on the message, so the model can be told what it had
    // been put to when it is asked again after the room was made.
    fn push_cancelled(&mut self, call: &ToolCall, thinking: &str, thinking_ms: u64, bound: usize) {
        self.push_stopped_call(call, thinking, thinking_ms, bound, false);
    }

    // A tool call the reader stopped with their own hand, while the model was
    // still writing it. The block of the words being written goes with the answer,
    // so what the reader stopped still stands in the chat: an answer that ends
    // with the model's last work hidden reads as though the program had thrown it
    // away.
    fn push_stopped_by_reader(
        &mut self,
        call: &ToolCall,
        thinking: &str,
        thinking_ms: u64,
        bound: usize,
    ) {
        self.push_stopped_call(call, thinking, thinking_ms, bound, true);
    }

    // The two roads above meet here. `by_reader` tells the call the reader stopped
    // from the call the room gave out under, in the chat and in what goes back to
    // the model.
    fn push_stopped_call(
        &mut self,
        call: &ToolCall,
        thinking: &str,
        thinking_ms: u64,
        bound: usize,
        by_reader: bool,
    ) {
        // The words of the call are kept to what the room left over can carry, and
        // the block the reader is shown is made from the same words: what the
        // reader sees and what the model is told cannot then differ.
        let args = head_chars(&call.arguments, bound);
        let (shown, _, target) = words_of(&call.name, &args);
        let shown = if target.is_empty() { shown } else { target };
        // A call stopped before it had a name has no words to show, and a block
        // with nothing under its head reads as a fault of the program.
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
                tool: String::from(if by_reader {
                    STOPPED_WORDS
                } else {
                    CANCELLED_WORDS
                }),
                mono: true,
                failed: true,
                cancelled: true,
                reader_stopped: by_reader,
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

    // Turn the open chat into the message list the API expects. The day and the
    // clock time of this machine stand at the head of the list, in a message of
    // the role `system`, once for the talk.
    fn build_history(&self) -> Value {
        let mut out: Vec<Value> = Vec::new();
        let Some(chat) = self.chat() else {
            return json!([]);
        };
        if let Some(line) = date_message(&self.cfg, began_of(chat)) {
            out.push(line);
        }
        let vision_on = self.pictures_can_go();
        // The records of the attachments of this chat stand in a folder of
        // its own room, so that is where their words and pictures are looked
        // for.
        let shelf = self.attach_place_for(chat);
        // The thoughts that may go back with the answers of the chat.
        let keep_thinking = self.kept_thinking();
        // Where the newest handoff summary that was really written stands among
        // the messages. The words before it are the words that summary stands
        // for, and they are left out here: that is the whole of the compaction.
        // The first message of the reader keeps its place whatever the summary
        // has to say, so its words, and the pictures hung on them, stay at the
        // front of the prompt. The server worked them through once and keeps the
        // work it did on them, which is why the summary rides after that first
        // message rather than in the place of it.
        let cut = last_summary_place(chat);
        for (place, message) in chat.messages.iter().enumerate() {
            // The first message of the reader always goes; the words a summary
            // stands for do not.
            if let Some(where_it_stands) = cut {
                if place != 0 && place < where_it_stands {
                    continue;
                }
            }
            match message.role.as_str() {
                // The handoff summary of a compaction, sent as the words of the
                // program beside the words of the reader. The mark of a summary
                // that was never written sends nothing of its own.
                "Summary" => {
                    if message.body.trim().is_empty() {
                        continue;
                    }
                    out.push(json!({
                        "role": "user",
                        "content": handoff_block(&message.body),
                    }));
                }
                // The words of the reader, and the attachments that hang on
                // them. Without attachments this is the plain words, as it
                // always was. The line of the day and the clock time is not put
                // among these words: it stands once, at the head of the talk.
                "You" => {
                    out.push(json!({
                        "role": "user",
                        "content": attach::content_at(&shelf, &message.body, &message.attachments, vision_on),
                    }));
                }
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
                    // A call that was stopped before the model had written the
                    // whole of it is not a call. The words of it stop in the
                    // middle, and a server set to read arguments out of them would
                    // fault over the broken JSON, as it faults over a call the room
                    // cut short in an answer. So the call goes back as the words of
                    // the turn it stood in, and the model is told in plain words
                    // that the call was cancelled and what it had written when it
                    // was stopped. Whether the room or the reader stopped it is
                    // told too, for one had its room made after it and the other
                    // had nothing after it at all.
                    if message.cancelled {
                        let told = if message.reader_stopped {
                            stopped_words(&message.tool_name, &message.tool_args)
                        } else {
                            cut_words(&message.tool_name, &message.tool_args)
                        };
                        out.push(json!({
                            "role": "assistant",
                            "content": told,
                        }));
                        // The telling of a call that was cut at the limit stands for
                        // the work the model was put to. Whatever of the talk it
                        // stands after -- the words of the turn it cut, and the
                        // thinking of that turn -- stands with it, and the words of
                        // the summary stand for them as much as for the rest of the
                        // talk. The cut is where the limit was come to, and it stands
                        // before the words it cuts off, so the model reads the
                        // summary and the words that came after it in the order the
                        // chat stands in.
                        if !message.reader_stopped {
                            let place = out.len();
                            if let Some(where_it_stands) = out.iter().rposition(|item| {
                                item.get("role").and_then(|r| r.as_str()) == Some("user")
                                    && item.get("content")
                                        .and_then(|c| c.as_str())
                                        .is_some_and(|said| said.starts_with(HANDOFF_FIRST))
                            }) {
                                let moved: Vec<Value> =
                                    out.drain(where_it_stands + 1..place).collect();
                                out.splice(where_it_stands + 1..where_it_stands + 1, moved);
                            }
                        }
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
                    // The words of the receipt, and after them the picture a tool
                    // opened, when there is one and this model can be shown a
                    // picture. With neither the entry is the plain words, as it
                    // always was.
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
        // Keep the open chat when it is still there, and forget the number of
        // the talk when it is not. A place is never opened by itself: the first
        // screen stands between places, and the reader opens a chat of the new
        // place from the side bar, or starts a new one.
        if self
            .current
            .as_ref()
            .map(|id| !self.chats.iter().any(|c| &c.meta.id == id))
            .unwrap_or(false)
        {
            self.current = None;
        }
        if self.current.is_none() {
            self.ctx_used = 0;
        }
        self.touch_structure();
    }

    // Start a new chat in the place that is open, and open it. Nothing is
    // written to disk yet: the first message the reader sends makes the chat a
    // real one and writes it, so a chat that was opened and left empty leaves
    // no file behind.
    //
    // A new chat stands where the reader left the chat before it. Every setter
    // of the two badges, the sandbox, the thinking mode and the choice of
    // keeping the thinking keeps the choice for the next chat in step with the
    // chat that stands open, so the new chat asks for the server the chat
    // before it ran on and the model on it stays loaded.
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
        self.ctx_used = 0;
        self.set_status("A new chat. Write in it below");
        self.touch_structure();
    }

    // Put the first screen up: no chat is open, so the mark, the line for this
    // hour and the chat box stand together in the middle of the window. This is
    // what the program shows when it starts, and what `Create new chat` in the
    // side bar goes back to.
    fn open_home(&mut self) {
        self.current = None;
        self.ctx_used = 0;
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

    // Point at a model when nothing sensible is picked yet. A model that was
    // picked is kept. Without this the top bar shows a model that no chat uses,
    // and a message then fails with "Choose a model in the top bar first".
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

    // Stand the thinking on a mode the model in hand offers. A model with
    // levels and a model with a switch do not offer the same words, so the
    // mode of a chat may mean nothing to the model that comes after it. A
    // mode the new model does not offer is put at "off", which every model
    // stands for: the badge then shows what the server will really be given,
    // and no word the template cannot read reaches its flags.
    fn normalize_thinking(&mut self) {
        let modes = self.thinking.modes();
        if modes.is_empty() {
            // A model with nothing to offer stands with the thinking off, and
            // sends no flag of it either way.
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

    // Put one mode on the chat in hand and on the next chat alike. A model
    // with no thinking at all takes both to "off".
    fn set_thinking_everywhere(&mut self, mode: &str) {
        if let Some(chat) = self.chat_mut() {
            chat.meta.thinking_mode = String::from(mode);
        }
        self.new_thinking = String::from(mode);
    }

    // What a command of the chat in hand may reach beyond its own folder. With no
    // chat open the choices the badges stand on are the choices of the next chat.
    fn access_of_open(&self) -> tools::Access {
        self.chat()
            .map(|chat| access_of(&chat.meta))
            .unwrap_or(self.new_sandbox_access)
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

    // The folder of the records of the open chat, for a picture a tool opened.
    fn attach_place(&self) -> Option<PathBuf> {
        self.chat().map(|chat| self.attach_place_for(chat))
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
            summarising: self.summarising,
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
            data_dir: config::paths().data.display().to_string(),
            sandbox_note: self.sandbox_note.clone(),
            chat_collapse_thinking: self.cfg.chat_collapse_thinking,
            chat_grow_thinking: self.cfg.chat_grow_thinking,
            chat_tools_in_thinking: self.cfg.chat_tools_in_thinking,
            chat_collapse_tools: self.cfg.chat_collapse_tools,
            chat_date_new_chat: self.cfg.chat_date_new_chat,
            chat_highlight: self.cfg.chat_highlight,
            chat_bold_markdown: self.cfg.chat_bold_markdown,
            chat_silent_retries: self.cfg.chat_silent_retries,
            chat_chapters: self.cfg.chat_chapters,
            chat_context_files: self.cfg.chat_context_files,
            mcp: self.mcp_snap(),
            chat_paste_limit: self.cfg.pasted_text_char_limit,
            ask_seconds: self.cfg.ask_seconds.min(config::GREATEST_ASK_SECONDS),
            ask_critical_seconds: self
                .cfg
                .ask_critical_seconds
                .min(config::GREATEST_ASK_SECONDS),
            chat_width: config::sanitize_chat_width(self.cfg.chat_width),
            // The Compaction section, so the sheet of the settings can be
            // filled in and the page can tell which way the talk is kept short.
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
            handoffs: chat
                .map(|one| handoff_marks(one, self.generating))
                .unwrap_or_default(),
            live_text: self.live_text.clone(),
            live_reasoning: self.live_reasoning.clone(),
            live_thought_ms: self.live_thought_ms,
            live_thought_done: self.live_thought_done,
            progress: self.progress,
            progress_on: self.progress_on,
            progress_images: self.progress_images,
            prompt_rate: self.prompt_rate,
            answer_rate: self.answer_rate,
            live_tools: self.live_tools.clone(),
            ask: self.asking.clone(),
            rev: self.rev,
            structure_rev: self.structure_rev,
        }
    }
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

    // The line the status stands on after the setting moved.
    fn words(self, on: bool) -> &'static str {
        match (self, on) {
            (SandboxSetting::Network, true) => "A command of this chat may reach the network",
            (SandboxSetting::Network, false) => "A command of this chat has no network",
            (SandboxSetting::Pip, true) => {
                "Python package installs are open for this chat, into the chat's own folder"
            }
            (SandboxSetting::Pip, false) => "Python package installs are off for this chat",
            (SandboxSetting::Fetch, true) => {
                "wget, curl and fetch may run in this chat, and reach the network"
            }
            (SandboxSetting::Fetch, false) => "wget, curl and fetch are off in this chat",
        }
    }
}

// What a command of this chat may reach, out of the marks the chat carries. A
// chat written before the three existed carries none of them, and reads as all
// three off.
pub fn access_of(meta: &store::ChatMeta) -> tools::Access {
    tools::Access {
        network: meta.sandbox_network,
        pip: meta.sandbox_pip,
        fetch: meta.sandbox_fetch,
    }
}

// What the side bar is made of. Every repository stands with the chats that
// are in it and were not put away, and the chats that belong to no repository
// stand on their own. A repository the reader pinned keeps its mark, and so
// does a chat; the page puts the pinned things in the group above the rest.
pub struct Lists {
    pub repositories: Vec<(Repo, Vec<Chat>)>,
    pub loose: Vec<Chat>,
    // The chats the reader put away, newest first, of every place.
    pub archived: Vec<Chat>,
}

impl Default for Lists {
    fn default() -> Self {
        Lists {
            repositories: Vec::new(),
            loose: Vec::new(),
            archived: Vec::new(),
        }
    }
}

// Put the chats of one group in the order the side bar shows them: the chat
// that was opened last first. A chat that was never opened stands where the
// time of its last message puts it, so a list of chats that nobody has opened
// since the program was started is still the newest first.
fn by_opened(chats: &mut [Chat]) {
    chats.sort_by_key(|c| std::cmp::Reverse(c.meta.opened.max(c.meta.updated)));
}

// The handle a window keeps.
pub struct Engine {
    // The state of the program. The window reaches it through the calls of this
    // file. The live run of the engine against a real model reaches it outright, to
    // switch the devices of the machine off, which no call of the program does: that
    // is the only reason the field stands open there, and in the tests beside it.
    #[cfg(any(test, feature = "live-driver"))]
    pub state: Arc<Mutex<State>>,
    #[cfg(not(any(test, feature = "live-driver")))]
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
            prompt_rate: 0.0,
            answer_rate: 0.0,
            rate_start: None,
            live_tools: Vec::new(),
            asking: None,
            thought_start: None,
            thought_end: None,
            // The meter of the context moves at most twice a second, so the
            // moment it last moved stands here.
            ctx_at: None,
            generating: false,
            summarising: false,
            busy: false,
            status: String::from("Ready"),
            ctx_total: 0,
            ctx_used: 0,
            ctx_fresh: false,
            last_opened: 0,
            sandbox_available,
            sandbox_note,
            thinking: markdown::ThinkingOptions::default(),
            rev: 0,
            structure_rev: 0,
            new_code: false,
            new_tools: store::ToolSwitches::default(),
            new_search: false,
            new_sandbox: true,
            // Every part of what a command may reach stands off for a new set of
            // settings, and for a chat written before the three existed.
            new_sandbox_access: tools::Access::default(),
            new_mcp: Vec::new(),
            mcp_tools: std::collections::BTreeMap::new(),
            mcp_fault: std::collections::BTreeMap::new(),
            context_slot: std::collections::BTreeMap::new(),
            context_holds: std::collections::BTreeMap::new(),
            new_thinking: String::from("off"),
            // llama.cpp keeps the thinking of the earlier turns on its own, so
            // the program asks for the same thing and lets the reader turn it
            // off for a model that offers the choice.
            new_preserve: true,
        };
        state.refresh_lists();
        // The talks go first, so a repository is found to hold no chat of the
        // model that works inside it. The records of the repositories go with
        // them, for a command of the terminal tool works in the folder of a
        // repository and has no business with them.
        state.store.migrate_chats_out();
        state.store.migrate_records_out();
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

    // Read the state without changing it.
    pub fn snapshot(&self) -> Snap {
        match self.state.lock() {
            Ok(state) => state.snap(),
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
            state.set_status("Nothing was written");
            return;
        }
        if state.generating {
            state.set_status("An answer is still being generated");
            return;
        }
        // A message sent from the first screen opens a chat of this place. The
        // window grows the chat box into a chat at the same time.
        if state.current.is_none() {
            state.start_chat();
        }
        // A chat that was cut for a handoff summary and never got one stands at
        // the place it was cut: its talk is past the room of the model and
        // cannot be sent as it is. This is said before aught else, for a reader
        // who presses send on such a chat is waiting to hear why the chat does
        // not go on. The reader starts the summary again, and the chat goes on
        // after it, so no word of the talk is lost and no model is asked to read
        // a talk it cannot hold.
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
        if state.server_binary().is_none() {
            state.set_status("llama-server was not found. Set its folder in the settings");
            return;
        }
        drop(state);
        self.cancel.store(false, Ordering::SeqCst);
        let _ = self
            .jobs
            .send(Job::Prompt(SentMessage { text, files }));
    }

    // Start the handoff summary of the open chat again. A summary the server
    // never finished leaves the chat standing at the place it was cut, and only
    // the reader says when to ask for it again: the program does not put a model
    // to work behind the back of a chat that is waiting.
    pub fn restart_handoff(&self) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if state.generating {
            state.set_status("An answer is still being generated");
            return;
        }
        // The word of the waiting chat comes first, so a reader who pressed the
        // mark of a summarisation that was already written is told so, and not
        // sent to look at the model. The model is looked at after it, for the
        // worker says the same thing when it has nothing to ask.
        if !state.handoff_owed() {
            state.set_status("No handoff summary is waiting for this chat");
            return;
        }
        if state.model().is_none() {
            state.set_status("Choose a model in the top bar first");
            return;
        }
        drop(state);
        self.cancel.store(false, Ordering::SeqCst);
        let _ = self.jobs.send(Job::Handoff);
    }

    // Stop reading the answer.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    // The tools of the open chat: the terminal tool, and the tools that work on
    // the files of the chat. One switch stands over all of them, for a reader who
    // does not want the model reaching the machine does not want it reaching the
    // files either. The name of the switch stays `code`, which is the name the
    // chat carries on disk and the name the pages send.
    pub fn set_code(&self, enabled: bool) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if let Some(chat) = state.chat_mut() {
            chat.meta.code_enabled = enabled;
        }
        // The same choice opens the next chat too, whatever stands open now.
        state.new_code = enabled;
        state.persist();
        state.set_status(if enabled {
            "The model may use the tools of the program"
        } else {
            "The tools of the program are off"
        });
    }

    // One line of the drop-down of the badge of the tools. The badge itself is
    // the switch over all of them, so a tool that is switched on here is offered
    // only while the badge stands on; turning this line off takes the tool out of
    // the chat even when the badge is on. The same choice opens the next chat.
    pub fn set_tool_switch(&self, part: &str, on: bool) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if let Some(chat) = state.chat_mut() {
            if !chat.meta.tools.set(part, on) {
                return;
            }
        }
        // The same choice opens the next chat too, whatever stands open now, as
        // with the badge of the tools itself.
        if !state.new_tools.set(part, on) {
            return;
        }
        state.persist();
        let word = tool_group_word(part);
        let words = if on {
            format!("The model may use {} again", word)
        } else {
            format!("{} are off", word)
        };
        state.set_status(&words);
    }

    // The web search of the open chat. It stands on a switch of its own and not
    // behind the badge of the tools, for it reads the web out of the program: a
    // command of the terminal tool cannot reach the web at all, since the jail of
    // it has no network. The name of the switch is `search` on every road it
    // travels on.
    pub fn set_search(&self, enabled: bool) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if let Some(chat) = state.chat_mut() {
            chat.meta.search_enabled = enabled;
        }
        // The same choice opens the next chat too, whatever stands open now.
        state.new_search = enabled;
        state.persist();
        state.set_status(if enabled {
            "The model may search the web"
        } else {
            "Web search is off"
        });
    }

    // One server of the reader's own, switched on or off for this chat and for
    // the next one. The servers of the reader's own are the tools of machines
    // and programs beyond this one, so they stand off until the reader opens
    // them, as the sandbox gates stand off.
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
        // The same choice opens the next chat too, whatever stands open now.
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
            format!("The tools of {} are offered to the model", name)
        } else {
            format!("The tools of {} are off", name)
        };
        state.set_status(&said);
    }

    // The servers of the reader's own, as they stand in the settings. The sheet
    // of the settings asks for these, for the state keeps no words of code.
    pub fn mcp_servers(&self) -> Vec<mcp::Server> {
        match self.state.lock() {
            Ok(state) => state.cfg.mcp_servers.clone(),
            Err(_) => Vec::new(),
        }
    }

    // Add one server, or write a new one over an old one. The words the reader
    // wrote are kept inside the bounds of `mcp::clean`, and a server with
    // nothing written in the place its kind asks for is refused. The tools of
    // the server are then read again, out of this call, so the sheet is not held
    // up by a server that takes its time.
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
        // What was known of this server is thrown away, for the server the
        // reader has just written is not the server the program looked at.
        state.mcp_tools.remove(&id);
        state.mcp_fault.remove(&id);
        let _ = config::save(&state.cfg);
        state.touch();
        state.set_status("The server is kept. Its tools are being read");
        drop(state);
        let _ = self.jobs.send(Job::McpLook(id));
        Ok(String::from("kept"))
    }

    // Take a server away: out of the settings, out of every chat that has it
    // switched on, and off the machine if it was a server of the program itself.
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
        state.set_status("The server was taken away");
        Ok(String::from("gone"))
    }

    // Whether a server stands in the drop-down of the badge. A server that is
    // taken out of the menu is kept, and the chats that had it switched on keep
    // that choice: the reader who takes a server out of the menu and puts it
    // back finds it where they left it.
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

    // Ask one server again what it has to offer. The answer comes in its own
    // time, and the sheet of the settings sees it when it comes.
    pub fn mcp_look(&self, id: &str) {
        let Ok(mut state) = self.state.lock() else { return };
        state.set_status("The server is being looked at");
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
        // The same choice opens the next chat too, whatever stands open now.
        state.new_sandbox = enabled;
        state.persist();
        state.set_status(if enabled {
            "Tool runs are sandboxed"
        } else {
            "Tool runs are not sandboxed"
        });
    }

    // One of the three settings of the sandbox, for the chat in hand and for the
    // next chat alike. They stand off until the reader opens them in the menu of
    // the sandbox badge, and they are settings of the jail: a chat with the
    // sandbox off runs its commands free and needs nothing opened.
    //
    // An install and a fetch are nothing without the network, so opening one of
    // them opens the network too. Closing one leaves the network open only while
    // the other still needs it, which is what the walls of the jail do as well.
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
        // The same choice opens the next chat too, whatever stands open now.
        match setting {
            SandboxSetting::Network => state.new_sandbox_access.network = on,
            SandboxSetting::Pip => state.new_sandbox_access.pip = on,
            SandboxSetting::Fetch => state.new_sandbox_access.fetch = on,
        }
        state.persist();
        state.set_status(setting.words(on));
    }

    // Choose the thinking mode of the open chat. A model with no thinking
    // mode does not change, and neither does a mode the model does not offer.
    pub fn set_thinking(&self, mode: &str) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if state.generating {
            state.set_status("Wait for the answer to finish");
            return;
        }
        if !state.thinking.modes().contains(&mode) {
            return;
        }
        // The mode the chat in hand stands on, or the one the next chat would
        // open with. A choice that did not move changes nothing, and the
        // running server keeps its flags and its loaded model with it.
        let changed = state.thinking_of_open() != mode;
        if let Some(chat) = state.chat_mut() {
            chat.meta.thinking_mode = mode.to_string();
        }
        // The same choice opens the next chat too, whatever stands open now.
        state.new_thinking = mode.to_string();
        state.persist();
        state.set_status(&format!("Thinking: {}", mode));
        state.touch();
        drop(state);
        if changed {
            // The flag is server level, so the running server goes with it. A
            // mode the chat wore already leaves the server, and the model, be.
            let _ = self.jobs.send(Job::ThinkingChanged);
        }
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
            state.set_status("Wait for the answer to finish");
            return;
        }
        if !state.thinking.has_preserve {
            state.set_status("This model has no setting for keeping the thinking");
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
            "The thinking of the earlier turns is kept"
        } else {
            "The thinking of the earlier turns is dropped"
        });
        state.touch();
        drop(state);
        // The flag stands in the command line of the server, so the running
        // server goes with it. A choice that did not move leaves the server,
        // and the model on it, be.
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
            state.set_status("Wait for the answer to finish");
            return;
        }
        if id == state.scope_repo {
            return;
        }
        state.scope_repo = id.clone();
        state.current = None;
        if let Some(mut repo) = state.repositories.iter().find(|r| r.id == id).cloned() {
            state.store.touch_repo(&mut repo);
            state.status = format!("Repository {}", repo.name);
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
            state.set_status("Wait for the answer to finish");
            return;
        }
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

    // The reader answered the question the model is waiting on. The words go into
    // the block of the run, and the ask goes away with them. A question that was
    // answered already, or that has been given over, is answered no more, and the
    // page is told so.
    pub fn answer_ask(&self, id: &str, answer: &str) -> bool {
        let Ok(mut guard) = self.state.lock() else {
            return false;
        };
        guard.answer_asking(id, answer.trim())
    }

    // One file the reader was offered, for the button of a block. The name is the
    // name the block carries, and is read as the tool reads it: only the room of
    // the open chat is opened, so a name that steps out of that room gives
    // nothing. The rule of the sandbox lies over a download as it lies over a
    // command. The answer is the name of the file, the kind of its bytes, and the
    // bytes.
    pub fn gift(&self, asked: &str) -> Option<(String, String, Vec<u8>)> {
        let workdir = self.open_workdir()?;
        let (name, bytes) = download::bytes_of(&workdir, asked)?;
        let (_, tail) = download::split_name(&name);
        Some((name, download::mime(&tail).to_string(), bytes))
    }

    // Several files the reader was offered, in one archive, for the button that
    // takes the whole of an offer at once. A name that is not in the room of the
    // chat is passed over; when none of them can be had, nothing comes back. The
    // archive is made in the memory and is never written out to the room of the
    // chat, so a download leaves no file of its own behind.
    pub fn gift_zip(&self, asked: &[String]) -> Option<(String, Vec<u8>)> {
        let workdir = self.open_workdir()?;
        let mut held: Vec<(String, Vec<u8>)> = Vec::new();
        for one in asked.iter().take(download::GREATEST_FILES) {
            if let Some((name, bytes)) = download::bytes_of(&workdir, one) {
                // A file of a folder stands in the archive under its own name, and
                // two files of the same name would otherwise write over one
                // another: the folder they came out of is put back on the second.
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

    // The room of the open chat, which is the only room a download is read from.
    fn open_workdir(&self) -> Option<PathBuf> {
        let Ok(state) = self.state.lock() else {
            return None;
        };
        state.chat().map(|chat| state.workdir_for(chat))
    }

    // Go back to the first screen, where a new chat is written. The chat box
    // stands in the middle of the window under the mark and the line for this
    // hour, which is the screen the program opens on. The chat itself is made
    // by the first message, so a new chat that is left alone leaves no file.
    pub fn new_chat(&self) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if state.generating {
            state.set_status("Wait for the answer to finish");
            return;
        }
        state.open_home();
        state.set_status("A new chat. Write in it below");
    }

    // Open one chat of the side bar, of any place. The place of that chat
    // becomes the place that is open, so the bar, a command of the tool and the
    // next chat all follow the chat the reader went to. The chat that was opened
    // last stands at the head of its group in the side bar.
    pub fn open_chat(&self, id: String) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if state.generating {
            state.set_status("Wait for the answer to finish");
            return;
        }
        // A chat of another place is read from disk, for the side bar lists
        // every place and the engine holds only the one that is open.
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
            // The chat goes with its repository, so the place is read again
            // with this chat kept open.
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
    // The side bar works on chats and repositories by their id, and the chat it
    // works on is seldom the chat that stands open. Each of these reads the chat
    // out of the place that holds it, changes the record, and writes it back.

    // Give one chat another name. A name of nothing is no name, and the chat
    // keeps the one it had.
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

    // Stand a chat in the `Pinned` group of the side bar, or take it out of it.
    // A pinned chat stands in that group alone, so it is not shown in the group
    // of its repository as well.
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

    // Put a chat away, or bring it back. A chat that is put away stands in no
    // group of the side bar; it is read again from the list of the archived
    // chats. A chat that is put away while it stands open leaves the window at
    // the first screen, for the talk it held is out of the way.
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
            "The chat is archived. Bring it back from Archived chats"
        } else {
            "The chat is no longer archived"
        };
        state.set_status(words);
    }

    // Move a chat into a repository, or out of every repository, which is what
    // the `Move to` line of the menu of a chat does. The record of the chat is
    // written in the folder of the place it goes to and taken out of the folder
    // it came from, so a chat never stands in two places.
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
        state.store.save_chat(&chat);
        state.store.delete_chat(&older, &id);
        // The place that is open is read again, whichever of the two the chat
        // came from or went to. The chat goes with its new place when it is the
        // chat that stands open.
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

    // Take one chat away for good. The window goes to the first screen when it
    // was the chat that stood open.
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
        state.store.delete_chat(&chat.meta.repo_id, &id);
        state.chats.retain(|c| c.meta.id != id);
        if state.current.as_deref() == Some(id.as_str()) {
            state.open_home();
        }
        state.touch_structure();
        state.set_status("The chat was removed");
    }

    // Take one message out of the talk, and everything that came after it. The
    // model answered a talk that no longer reads that way, so the answer goes
    // with the message that led to it: a message of the reader takes the answer
    // it provoked along. A message of the model leaves the question behind, for
    // the program does not throw away what a reader wrote behind their back.
    pub fn remove_message(&self, id: String) {
        let Ok(mut state) = self.state.lock() else { return };
        if state.generating {
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
        // The records of the files hung on the messages that go are taken out of
        // the shelf of the chat with them, so no file stands behind with no
        // message to carry it. The shelf is named while the chat still stands
        // whole, for naming it needs the chat for a moment of its own.
        let gone: Vec<String> = {
            let Some(chat) = state.chat() else { return };
            chat.messages[index..]
                .iter()
                .flat_map(|one| one.attachments.iter().map(|a| a.id.clone()))
                .collect()
        };
        let room = state
            .chat()
            .map(|chat| state.attach_place_for(&chat))
            .unwrap_or_default();
        if let Some(chat) = state.chat_mut() {
            chat.messages.truncate(index);
            chat.meta.updated = store::now_secs();
        }
        if !gone.is_empty() {
            attach::remove_at(&room, &gone);
        }
        // A chat left with nothing of the talk is deleted as a chat that was
        // never filled is. A talk of notes alone is no talk: a note is written
        // for the window, and says nothing the model was told.
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
            state.set_status("The message was removed. The chat held nothing left, so it went too");
            return;
        }
        state.persist();
        state.touch();
        state.set_status("The message was removed, with everything after it");
    }

    // Write the answer of a message again. A message of the reader keeps its
    // place, and the talk is cut back after it; a message of the model goes with
    // the answer that came after it. The ask is then the talk as it stands, which
    // is the shape of a message the reader has just sent.
    pub fn retry_message(&self, id: String) {
        let Ok(mut state) = self.state.lock() else { return };
        if state.generating {
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
        // Only a message of the reader, or an answer of the model, has an answer
        // to be written again. A run of a tool, a note and a mark of a chapter
        // are not asks of the reader, and the page offers nothing over them.
        let cut = match role.as_str() {
            "You" => index + 1,
            "Assistant" => index,
            _ => {
                state.set_status("That message has no answer to be written again");
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
            .map(|chat| state.attach_place_for(&chat))
            .unwrap_or_default();
        if let Some(chat) = state.chat_mut() {
            chat.messages.truncate(cut);
            chat.meta.updated = store::now_secs();
        }
        if !gone.is_empty() {
            attach::remove_at(&room, &gone);
        }
        // The answer that stands under the mark of a stop is gone with the rest,
        // so the mark goes too.
        if let Some(chat) = state.chat_mut() {
            clear_stopped_marks(chat);
        }
        state.persist();
        state.touch();
        drop(state);
        self.cancel.store(false, Ordering::SeqCst);
        let _ = self.jobs.send(Job::Retry);
    }

    // Go on with an answer the reader stopped. Only an answer of the model that
    // carries the mark of the stop may be gone on with, and only when nothing is
    // coming and no handoff summary is owed. The ask is the talk as it stands:
    // the words of the stopped answer are the last message of the model, so the
    // template puts them under the mark that opens an answer of a model and leaves
    // that mark open, and the model writes on from the last word it had put down.
    pub fn continue_message(&self, id: String) {
        let Ok(mut state) = self.state.lock() else { return };
        if state.generating {
            state.set_status("An answer is still being generated");
            return;
        }
        if state.handoff_owed() {
            state.set_status(
                "The handoff summary of this chat was never written. Start it again from the summarisation timeline",
            );
            return;
        }
        let (role, stopped) = {
            let Some(chat) = state.chat() else {
                state.set_status("That message is not in this chat");
                return;
            };
            let Some(index) = chat.messages.iter().position(|one| one.id == id) else {
                state.set_status("That message is not in this chat");
                return;
            };
            (chat.messages[index].role.clone(), chat.messages[index].stopped)
        };
        if role != "Assistant" || !stopped {
            state.set_status("Only an answer you stopped can be gone on with");
            return;
        }
        // The words to go on from must still be at the end of the talk. The room
        // made for a summarisation can have taken them away with the words a
        // handoff summary stands for, and can have left a call the model asked for
        // standing there instead.
        let open = ends_open(&state.build_history());
        if !open {
            state.set_status(
                "The words of that answer are no longer at the end of the talk, so they cannot be gone on with",
            );
            return;
        }
        // The model is looked at last, for the marks under a message are asked
        // about before the model is: a reader who pressed a mark that this talk
        // cannot answer is told that first.
        if state.model().is_none() {
            state.set_status("Choose a model in the top bar first");
            return;
        }
        // The mark of the stop goes off as the answer is gone on with: what comes
        // now is the end of that answer, and a second press would go on with it
        // twice.
        if let Some(chat) = state.chat_mut() {
            clear_stopped_marks(chat);
        }
        state.persist();
        state.touch();
        drop(state);
        self.cancel.store(false, Ordering::SeqCst);
        let _ = self.jobs.send(Job::Resume(id));
    }

    // Give a repository another name. The folder of a repository is named after
    // it, so the folder is moved with the record, the Python packages of the
    // chats and the records of the attachments in it. The id does not move, so
    // the talks of the repository and the chats that point at it are not cut
    // loose.
    pub fn rename_repository(&self, id: String, name: String) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if state.generating {
            state.set_status("Wait for the answer to finish");
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

    // Stand a repository in the `Pinned` group of the side bar, or take it out
    // of it. The chats of the repository are not moved.
    pub fn set_repository_pinned(&self, id: String, on: bool) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        let Some(mut repo) = state.repositories.iter().find(|r| r.id == id).cloned() else {
            state.set_status("That repository is not there");
            return;
        };
        repo.pinned = on;
        state.store.save_repo(&repo);
        state.repositories = state.store.scan_repos();
        state.touch_structure();
        let words = if on {
            "The repository stands in Pinned"
        } else {
            "The repository is no longer pinned"
        };
        state.set_status(words);
    }

    // Every repository with the chats that stand in it, the chats that belong
    // to no repository, and the chats the reader put away. The side bar asks for
    // this when a list changed, and not on every look at the state, for the
    // chats are read from the disk.
    pub fn lists(&self) -> Lists {
        let Ok(state) = self.state.lock() else {
            return Lists::default();
        };
        state.lists()
    }

    // Pick another model. The worker stops the running server.
    pub fn select_model(&self, id: String) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if state.generating {
            state.set_status("Wait for the answer to finish");
            return;
        }
        state.cfg.model_id = id.clone();
        if let Some(chat) = state.chat_mut() {
            chat.meta.model_id = id.clone();
            state.persist();
        }
        let _ = config::save(&state.cfg);
        // The new model reads its own template, so the thinking may stand on
        // a word the new model has never heard of: a level gives place to the
        // switch, and the switch to a level. The badge is moved to a mode the
        // new model offers before the worker has even seen this call, so the
        // reader never sees a mark against a mode that is not there.
        state.refresh_thinking();
        state.normalize_thinking();
        state.persist();
        state.touch_structure();
        let _ = self.jobs.send(Job::ModelChanged);
    }

    // Store new settings, and refresh what depends on them.
    pub fn save_settings(&self, cfg: Config) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if state.generating {
            state.set_status("Wait for the answer to finish");
            return;
        }
        let mut cfg = cfg;
        // The chat box turns a long paste of words into a file, and the number
        // it does this at is kept inside the two bounds here.
        cfg.pasted_text_char_limit = config::sanitize_pasted_text_limit(cfg.pasted_text_char_limit);
        cfg.chat_width = config::sanitize_chat_width(cfg.chat_width);
        // Compaction. The two words of the sheet are kept to the two each has,
        // the two numbers are kept inside their bounds, and the handoff prompt
        // is kept short enough to be sent beside a whole talk.
        cfg.chat_compaction = config::sanitize_compaction(&cfg.chat_compaction);
        cfg.chat_compact_by = config::sanitize_compact_by(&cfg.chat_compact_by);
        cfg.chat_compact_percent = config::sanitize_compact_percent(cfg.chat_compact_percent as i64);
        cfg.chat_compact_reserve = config::sanitize_compact_reserve(cfg.chat_compact_reserve as i64);
        cfg.chat_compact_prompt = config::sanitize_handoff_prompt(&cfg.chat_compact_prompt);
        cfg.chat_compact_prompt_short =
            config::sanitize_handoff_prompt_short(&cfg.chat_compact_prompt_short);
        state.cfg = cfg;
        let _ = config::save(&state.cfg);
        state.store = store::Store::new(config::repositories_dir(&state.cfg));
        // The reader may have pointed the program at a folder of repositories
        // that still holds its talks, and its records, inside them.
        state.store.migrate_chats_out();
        state.store.migrate_records_out();
        state.refresh_lists();
        state.reload_chats();
        state.set_status("Settings saved");
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
            mcp: Vec::new(),
            model_name: String::new(),
            model_kind: String::new(),
            in_repository: false,
            repository_id: String::new(),
            repository_name: String::new(),
            chat_title: String::new(),
            context_used: 0,
            context_total: 0,
            sandbox_access: tools::Access::default(),
            generating: false,
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
            data_dir: String::new(),
            sandbox_note: String::new(),
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
            ask: None,
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
                stop_server(&state, &mut server);
                return;
            }
        };
        match job {
            Job::Shutdown => {
                stop_server(&state, &mut server);
                return;
            }
            Job::ModelChanged | Job::SettingsChanged => {
                stop_server(&state, &mut server);
                if let Ok(mut state) = state.lock() {
                    state.ctx_total = 0;
                    state.refresh_thinking();
                    // The modes of the new model are known by now, so a mode
                    // the chat wore that the model cannot read is put at "off"
                    // before the next answer asks for the flags of the server.
                    state.normalize_thinking();
                    let name = state
                        .model()
                        .map(|m| m.name)
                        .unwrap_or_else(|| String::from("The model"));
                    let text = format!("{} loads with the next message", name);
                    state.set_status(&text);
                }
            }
            Job::ThinkingChanged => {
                // The flags are server level, so the running server goes.
                // The model and context stay the same, so nothing else changes.
                stop_server(&state, &mut server);
            }
            Job::Prompt(sent) => {
                let mut turn = Turn::Sent(sent);
                // How many times the model has been asked to go on with its work
                // after a handoff summary of this one send.
                let mut goings_on = 0usize;
                loop {
                    let ended = run_turn(&state, &mut server, &cancel, turn);
                    // The answer is in the chat and the room it took is known, so
                    // this is the moment a handoff summary is asked for, if the
                    // talk has come that far. It is asked for whatever the answer
                    // came to, for a chat left past the limit is only stopped
                    // again on the next message.
                    let made_room =
                        maybe_compact(&state, &mut server, &cancel, Weigh::Talk);
                    // A summary the program asked for is written so that the work
                    // goes on, and so it does: the model is put back to its talk
                    // with the first prompt of the reader and the summary in front
                    // of it, and none of the words the summary stands for. An
                    // answer the reader stopped, or one the stream broke, is not
                    // gone on with.
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
            }
            Job::Retry => {
                // The talk was cut back to the message the reader is going on
                // from, so the ask is the talk as it stands. The rules of the
                // room and of the handoff summary are the same as for a message
                // of the reader.
                let mut turn = Turn::Retry;
                let mut goings_on = 0usize;
                loop {
                    let ended = run_turn(&state, &mut server, &cancel, turn);
                    let made_room = maybe_compact(&state, &mut server, &cancel, Weigh::Talk);
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
            }
            Job::Resume(id) => {
                // The words the reader stopped stand at the end of the talk, so the
                // ask is the talk as it stands and the model writes on from them.
                // No second ask follows: an answer without a command is an answer
                // at its end, and the room of the talk is weighed afterwards as it
                // is after every other answer.
                run_turn(&state, &mut server, &cancel, Turn::Resume(id));
                let _ = maybe_compact(&state, &mut server, &cancel, Weigh::Talk);
            }
            Job::McpLook(id) => {
                // One server of the reader's own is asked what it has to offer.
                // The state lock is let go of for the while, for a server that
                // will not answer is waited for, and the window must go on
                // drawing while it is waited for.
                let wanted = match state.lock() {
                    Ok(guard) => guard
                        .cfg
                        .mcp_servers
                        .iter()
                        .find(|server| server.id == id)
                        .cloned(),
                    Err(_) => break,
                };
                let Some(server) = wanted else { continue };
                let found = mcp::list(&server);
                let Ok(mut guard) = state.lock() else { break };
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
            Job::Handoff => {
                // The reader asked for a summary that never came to be written
                // again. When it comes, the model is put back to work, for that is
                // what a handoff summary is written for, and the chat was left
                // standing at the mark of it.
                let made = run_handoff(&state, &mut server, &cancel);
                if made && !cancel.load(Ordering::Relaxed) {
                    run_turn(&state, &mut server, &cancel, Turn::Continue);
                }
            }
        }
    }
}

// The name of the file that the worked-through talk of one chat stands in,
// inside the folder of the program. The server takes the name and looks for it
// in that folder and in no other place, and it throws out a name that holds a
// mark of a folder, a colon, a question mark, a quote, or a character no word
// is made of. So the id of the chat is kept as far as it is made of marks a
// name of a file may hold, and the id of the model -- which is a path of the
// disk, and so could never stand in a name at all -- is hashed into a few marks
// at the end instead.
//
// The two belong in one name, for the talk of a chat read through by one model
// is not the talk of the same chat read through by another. The words are the
// same words, but what the model made of them, and the shape the server put
// them into, are not: a talk of one model laid in a slot of another is thrown
// out by the server as a talk that does not fit, so the two are kept apart by
// the name and never mixed.
pub fn context_name(chat_id: &str, model_id: &str) -> String {
    let mut name = String::new();
    for mark in chat_id.chars() {
        if mark.is_ascii_alphanumeric() || mark == '.' || mark == '_' || mark == '-' {
            name.push(mark);
        }
    }
    if name.is_empty() {
        name.push_str("chat");
    }
    // Two hundred marks of the id, sixteen of the hash of the model, the mark
    // between, and the four of the ending: well within the two hundred and
    // fifty-five the server reads. An id longer than that is not a chat id of
    // this program, but it is no reason either to let the talk of one chat fall
    // into the file of another whose id runs the same way for two hundred marks,
    // so a long id is not cut: the whole of it goes into a hash of its own,
    // which tells every chat apart and is short enough to stand.
    if name.len() > 200 {
        name = format!("{:016x}", fnv1a(chat_id));
    }
    format!("{}-{:016x}.ctx", name, fnv1a(model_id))
}

// FNV-1a: the plainest hash there is, and the shortest to write out. It is here
// to keep a name short and even, and to tell the talks of two models apart.
// Nothing of it stands against a hurt that is meant.
fn fnv1a(words: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in words.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

// The slot of the running server that the chat in hand wants, with its talk
// laid in that slot again.
//
// An ask that names no slot goes to whatever slot the server has free, and the
// server reads the talk through from its first word unless the front of it
// happens to match what a slot already holds -- which, between two chats that
// ask different things of the model, it does not. An ask that names the slot
// the talk was worked through in is stood on: the server keeps the work it has
// already done, and works through only the words that came after it.
//
// So a chat that comes back is given the slot it had, and the talk of it is
// taken out of its file and laid in the slot first. A chat that stands in the
// slot we are taking has its talk written out to its own file before the slot
// is gone over, so nothing of it is lost and it is read back in its turn.
//
// None is given back when the keeping is switched off, when no chat stands
// open, or when the state is gone: the ask then goes out with no slot named,
// exactly as it always did.
fn context_claim(state: &Arc<Mutex<State>>, base_url: &str, model_id: &str) -> Option<usize> {
    let (chat_id, slots) = {
        let Ok(guard) = state.lock() else { return None };
        if !guard.cfg.chat_context_files {
            return None;
        }
        // A chat that is not open has no talk to keep, and no id to name its
        // file by.
        let chat_id = match guard.current.as_ref() {
            Some(id) if !id.is_empty() => id.clone(),
            _ => return None,
        };
        // The server has one slot for each path it may work a talk through at a
        // time, and the slots are named from nought.
        let slots = guard
            .model()
            .map(|m| guard.settings_for(&m).parallel)
            .unwrap_or(1)
            .max(1) as usize;
        (chat_id, slots)
    };
    // Whether this chat had a slot before, and whether that slot still stands
    // for it. Both true is the common case: every turn of one chat after the
    // first comes here, and goes straight back with the slot it had. Nothing is
    // read from a file and nothing is written to one, for the server holds the
    // newest of the talk already.
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
    // The slot this chat had, so long as a server with this few paths has it
    // still. A server started with fewer paths than the one before has fewer
    // slots, and a slot of the older server is simply not there.
    let slot = match had {
        Some(slot) if slot < slots => slot,
        _ => {
            // The first slot nobody stands in. When every slot stands full of
            // the talk of another chat there is none such, and nought is taken:
            // the chat turned out of it has its talk written out just below, and
            // reads it back when it comes round again.
            (0..slots)
                .find(|one| !holds.contains_key(one))
                .unwrap_or_default()
        }
    };
    // The chat that stood in this slot, when it was another chat, has its talk
    // written out here. This is the moment the keeping earns its keep: the slot
    // of that chat is taken, and what the model made of its talk stands
    // nowhere else.
    if let Some(other) = holds.get(&slot) {
        if other != &chat_id {
            let name = context_name(other, model_id);
            let _ = llama::slot_state(base_url, "save", slot, &name);
        }
    }
    // The talk of this chat, out of its file and into the slot. A file that is
    // not there -- the first ask of a chat, or a folder the reader threw away --
    // is answered by the server with a fault, and the talk is then read through
    // as it always was. Nothing is said either way, for the words that come
    // back are the same words however the talk got there.
    let name = context_name(&chat_id, model_id);
    let _ = llama::slot_state(base_url, "restore", slot, &name);
    if let Ok(mut guard) = state.lock() {
        guard.context_holds.insert(slot, chat_id.clone());
        guard.context_slot.insert(chat_id, slot);
    }
    Some(slot)
}

// Write out every talk that stands worked-through in the running server, each
// to its own file. The server is alive when this is called, and dead directly
// after it, so this is the last moment there is to write them at. Nothing is
// said when one of them cannot be written: a talk that was not kept is a talk
// read through again, and the words of a chat stand in its file of chats as
// ever -- only the reading of them has to be done twice.
fn save_all_contexts(state: &Arc<Mutex<State>>, base_url: &str, model_id: &str) {
    let holds = match state.lock() {
        Ok(guard) => guard.context_holds.clone(),
        Err(_) => return,
    };
    for (slot, chat_id) in holds {
        let name = context_name(&chat_id, model_id);
        let _ = llama::slot_state(base_url, "save", slot, &name);
    }
}

// Stop the running server. The talks that stand worked-through in it are
// written out first, for the server is the one place they stand, and after this
// call there is nothing left to write them from: this is the unload of the
// model, whether it comes from the reader changing the model or the settings,
// or from the program being shut down. The bookkeeping of the slots goes with
// the server, for the next server has slots of its own and would not
// understand a talk of the one before.
fn stop_server(state: &Arc<Mutex<State>>, server: &mut Option<Server>) {
    if let Some(old) = server.as_ref() {
        save_all_contexts(state, &old.url, &old.model_id);
    }
    if let Ok(mut guard) = state.lock() {
        guard.context_lost();
    }
    if let Some(mut old) = server.take() {
        old.stop();
    }
}

// The words added to the handoff prompt of the reader. The talk being
// summarised is full of the calls of tools, for that is what a working session
// is made of, and a model that reads them again is minded to make one of its
// own. The tools stand offered in the ask because they stand offered in the talk
// -- the list of them is part of the front of the words the server has already
// worked through, so it cannot simply be left out -- which is why the ask has to
// say out loud that there is nothing here to be called. A model that asks
// over is answered by `summary_calls_answered` and asked again.
pub const SUMMARY_GUARD: &str = " Do not call any tools. The tools stand offered in this ask only because the talk above stands full of them: nothing of it is to be worked on now. Answer in plain text, with the summary itself only, and end it within three hundred words. Write the summary, never a call and never the marker of one. Do not speak of the summary, of the session, or of what you are about to do: the words you write are read by another model in place of the talk.";

// The ask for a summary carries no ceiling of its own. A summary is the making
// of the room and not the spending of it, so the model is given all of the room
// that stands over after the words of the talk and may write as long as it
// likes. What the ceiling did before was to keep the ask inside the window, and
// the window is kept by other means: an ask that runs to the end of the window
// is made again for a shorter summary. The number is sent as -1 on purpose, for
// a `--n-predict` out of the settings of the model would else cut the summary
// short as well.
pub const NO_SUMMARY_CEILING: i64 = -1;

// The words put after the telling of a call that was cut short, when that telling
// is the last thing of the talk. The telling asks the model for the call again, so
// the ask has to end on a word of the reader and not on the telling itself: a
// server set to read calls out of an answer writes one behind it, and the ask that
// made that call is thrown away with the call, which leaves no answer of the model
// at all. Nothing of these words is written into the chat, for the reader never
// wrote them.
pub const CUT_CONTINUE_WORDS: &str = "The space above was written by the program, and the call it tells of never ran. Do not answer it and do not write it again. Go on with the work from the point it reached before that call, and write the call only when the work still needs it and in parts small enough for the space you have. If the work is done, say so in one line and stop.";

// The word a talk that ends on the telling of a cut call is asked with. The talk
// is read as it stands, so the telling is the last thing of it whenever a call was
// stopped and nothing else came after it: the words of an answer, the mark of a
// summarisation and the thinking of the turn all stand before it.
pub fn cut_asks_for_a_call(history: &Value) -> bool {
    ends_on_a_call(history)
        && history
            .as_array()
            .and_then(|items| items.last())
            .and_then(|last| last.get("content"))
            .and_then(|c| c.as_str())
            .is_some_and(|said| said.starts_with(CUT_TOLD_FIRST))
}

// How much room the talk takes when the telling of a call that was stopped where
// the model wrote it is left out of it. The telling is the longest message of a
// talk that came to the limit in the middle of a call, for it holds the words of
// the call the model had written; and it is not words of the talk, for it is the
// program speaking, and the summary is written of the talk. So the room a
// summarisation is weighed against is the room of the talk without it. The ask
// for the summary itself carries the talk as it stands, telling and all, for the
// sake of the work the server has already done on the front of the words: see
// `summary_ask`.
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

// The ask for a handoff summary: the talk exactly as it stands, and the ask for
// the summary after it as a message of the reader.
//
// Nothing of the talk is changed for the ask. It used to be made over for it --
// the pictures taken out of it, the calls of tools and the receipts of them
// turned into lines of words -- and every one of those cuts fell into the middle
// of the words the server had already worked through. A server keeps the work it
// did on the front of a talk, and it can only keep it while the front of the
// next ask is word for word the front of the last: a talk made over is a talk
// worked through again from its first word, which spends the very room the
// summarisation was meant to save. So the talk goes out as it stands and the ask
// rides after it the way a message of the reader rides after it. Only the ask is
// new, and only the ask is worked through.
//
// The tools stand offered as they stand for an answer of this chat, for the same
// reason: the list of them is rendered into the front of the talk. A model that
// asks for one while it is writing the summary has the call cancelled and is
// asked again -- see `ask_summary_for_handoff`.
//
// No ceiling stands over the words the model may write: see NO_SUMMARY_CEILING.
pub fn summary_ask(history: &Value, prompt: &str, tools: Option<&Value>) -> Value {
    let mut items: Vec<Value> = history.as_array().cloned().unwrap_or_default();
    let mut said = String::from(prompt);
    said.push_str(SUMMARY_GUARD);
    items.push(json!({ "role": "user", "content": said }));
    let mut body = json!({
        "messages": Value::Array(items),
        "stream": true,
        "stream_options": { "include_usage": true },
        // And the server says on every piece how far it is with the words it was
        // sent, so the gauge above the chat box has numbers before an answer.
        "return_progress": true,
        "timings_per_token": true,
        // The model may write as long as it wants. A ceiling here cut the
        // summary short in the middle of a sentence, which is what the second
        // ask is there to mend.
        "max_tokens": NO_SUMMARY_CEILING,
    });
    // The tools of the chat, in the shape they are offered in for an answer, so
    // that the front of this ask is the front of the talk.
    if let Some(list) = tools {
        body["tools"] = list.clone();
        body["tool_choice"] = json!("auto");
    }
    body
}

// How many times the ask for a summary is made again when the model answers it
// with a call of a tool instead of with the summary. Twice is the most: a model
// that asks a third time has made its choice, and the words it wrote beside the
// calls are taken for the summary rather than spending more of the room the
// summary was come to make.
const SUMMARY_CALL_TRIES: usize = 2;

// The receipt given back to a call of a tool that came in the middle of the
// writing of a handoff summary. The call never ran, and these words say why.
pub const SUMMARY_NO_TOOL_WORDS: &str = "No tool may be called while a handoff summary is being written. The summary is read by another model in place of the talk, and nothing of the talk is worked on now. This call was cancelled and nothing of what it asked for was done. Write the summary in plain words.";

// The entries that answer the calls a model made instead of writing a summary:
// the saying of the model that held the calls, and one receipt for each of them
// saying it was cancelled and why. They are shaped as every other run of a talk
// is shaped, so the server reads a run that ended and owes nothing more.
pub fn summary_calls_answered(calls: &[llama::ToolCall], said: &str) -> Vec<Value> {
    // The id of a call is how a receipt is tied to it. A server that wrote no id
    // is given one here, and the same one on both sides of the pair.
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
    // The words the model wrote before it asked are kept with the asking, as they
    // are in every turn of a talk, so nothing it said is lost from the ask.
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

// Ask for the handoff summary of a talk, and keep asking while the model answers
// with a call of a tool instead of with the summary. Every ask carries the talk
// as it stands, so the work the server has already done on it stands from the
// first ask to the last: a cancelled call is answered where it stands and the
// model is asked on from there, which costs the few words of the answer and
// nothing of the talk.
fn ask_summary_for_handoff(
    state: &Arc<Mutex<State>>,
    base_url: &str,
    history: &Value,
    prompt: &str,
    tools: Option<&Value>,
    slot: Option<usize>,
    cancel: &Arc<AtomicBool>,
) -> SummaryShot {
    let mut body = summary_ask(history, prompt, tools);
    // The ask goes to the slot the talk above it was worked through in, so the
    // server stands on that work and works through the ask alone.
    if let Some(slot) = slot {
        body["id_slot"] = json!(slot);
    }
    let mut shot = ask_summary(state, base_url, &body, cancel);
    let mut tries = 0usize;
    while !shot.calls.is_empty()
        && tries < SUMMARY_CALL_TRIES
        && !shot.stopped
        && shot.error.is_none()
        && !cancel.load(Ordering::Relaxed)
    {
        tries += 1;
        tell(
            state,
            "The summary asked for a tool. The call was cancelled and the summary is asked for again",
        );
        if let Some(items) = body.get_mut("messages").and_then(|m| m.as_array_mut()) {
            items.extend(summary_calls_answered(&shot.calls, &shot.text));
        }
        // What the model gives after a cancelled call is the summary. The words
        // of the ask that was cancelled stand with it above, so nothing is said
        // twice.
        shot = ask_summary(state, base_url, &body, cancel);
    }
    shot
}

// The tools of the ask for a summary: the same list, in the same shape, as the
// list an answer of this chat is asked with. The list of the tools is rendered
// into the front of the talk, so an ask that carried another list -- or none, as
// this ask used to -- is a talk worked through again from its first word.
fn summary_tools(state: &Arc<Mutex<State>>) -> Option<Value> {
    let (code_on, search_on, chapters_on, access, switches, mcp_tools) = {
        let Ok(guard) = state.lock() else { return None };
        let chat = guard.chat();
        (
            chat.as_ref().map(|c| c.meta.code_enabled).unwrap_or(false),
            chat.as_ref().map(|c| c.meta.search_enabled).unwrap_or(false),
            guard.cfg.chat_chapters,
            chat.as_ref().map(|c| access_of(&c.meta)).unwrap_or_default(),
            chat.as_ref().map(|c| c.meta.tools.clone()).unwrap_or_default(),
            guard.mcp_definitions(),
        )
    };
    offered(code_on, search_on, chapters_on, access, &mcp_tools, &switches)
}

// One ask of the server for a handoff summary, and what came of it.
struct SummaryShot {
    // The words of the answer as they came in, before the envelope of them was
    // taken off.
    text: String,
    // Why the server stopped writing. "length" says that the room ran full under
    // the summary, which is the moment the second ask is made.
    finish: String,
    // The calls of tools the model made instead of writing the summary. None of
    // them ever runs: see `summary_calls_answered`.
    calls: Vec<llama::ToolCall>,
    // The reader stopped the ask with their own hand.
    stopped: bool,
    // The fault of the stream, when there was one.
    error: Option<String>,
}

// The mark that says the program is writing a handoff summary. It is taken off
// on the way out of the ask, whichever way out that is, so no fault of a server
// leaves the window waiting for words that are never shown.
struct SummaryWriting {
    state: Arc<Mutex<State>>,
}

impl Drop for SummaryWriting {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.state.lock() {
            guard.summarising = false;
            guard.touch();
        }
    }
}

// Whether the handoff summary has to be asked again for a short one. The room
// ran full under it, so its last sentence stops in the middle of itself, and the
// model is asked once more with the short prompt of the settings. A stop of the
// reader's own and a fault of the stream are never asked again for a shorter
// summary: the first has nothing to shorten, and the other is told to the reader.
fn summary_too_long(shot: &SummaryShot) -> bool {
    shot.error.is_none() && !shot.stopped && shot.finish == "length"
}

// Say a word to the reader about the summary, from outside the lock.
fn tell(state: &Arc<Mutex<State>>, words: &str) {
    if let Ok(mut guard) = state.lock() {
        guard.set_status(words);
        guard.touch();
    }
}

// Ask the server for a handoff summary once, with the words the model is to be
// given, and gather what it writes.
//
// None of it is shown. The words of a summary are not a saying of the model to
// the reader: they are read by the next model, and a wall of words laid over the
// talk is nothing to read. So nothing goes into the live text the window draws.
// The gauge above the chat box and the meter of the talk still move, for they
// say that the program is at work without saying a word of what it wrote.
fn ask_summary(
    state: &Arc<Mutex<State>>,
    base_url: &str,
    body: &Value,
    cancel: &Arc<AtomicBool>,
) -> SummaryShot {
    {
        let Ok(mut guard) = state.lock() else {
            return SummaryShot {
                text: String::new(),
                finish: String::new(),
                calls: Vec::new(),
                stopped: true,
                error: Some(String::from("The program state is gone")),
            };
        };
        // The gauge stands for this ask alone. The words under the number follow
        // the pictures the talk carries, for the server has none of them and the
        // count is read from the words that go out.
        guard.progress = 0;
        guard.progress_images = pending_images(&body["messages"]);
        guard.progress_on = true;
        guard.prompt_rate = 0.0;
        guard.answer_rate = 0.0;
        guard.rate_start = None;
        guard.thought_start = None;
        guard.thought_end = None;
        guard.live_thought_ms = 0;
        guard.live_thought_done = false;
    }
    // A summary is asked for with no hand from the program over it: it is the
    // making of the room and not the spending of it, and to stop a summary at the
    // limit would be to cut off the very words the limit was come to.
    let no_cut = std::sync::atomic::AtomicBool::new(false);
    // The words of the summary are gathered here, out of the sight of the window.
    let mut said = String::new();
    let gauge_state = state.clone();
    let context_state = state.clone();
    let answer = llama::stream_chat(
        base_url,
        body,
        cancel,
        &no_cut,
        &mut |piece: &str| {
            said.push_str(piece);
            // The first word is here, so the words that were sent are worked
            // through and the gauge above the chat box has nothing left to say.
            if let Ok(mut guard) = gauge_state.lock() {
                if finish_progress(&mut guard) {
                    guard.touch();
                }
            }
        },
        // The thinking of a summary is hidden with the rest of it.
        &mut |_piece: &str| {},
        &mut |made: llama::Progress| {
            if let Ok(mut guard) = gauge_state.lock() {
                let percent = (made.part() * 100.0).round() as u32;
                if percent != guard.progress {
                    guard.progress = percent;
                    // The whole of the words the ask carried are worked through,
                    // so the gauge has nothing left to say. The model is writing
                    // the summary now, and none of that writing is shown: the
                    // three dots under the heading of the summarisation say that
                    // the program is still at work. A gauge that stood at 100 per
                    // cent read as a fault of the program, for it said that the
                    // words were still being worked through.
                    if percent >= 100 {
                        finish_progress(&mut guard);
                    }
                    guard.touch();
                }
            }
        },
        &mut |made: llama::Context| {
            if let Ok(mut guard) = context_state.lock() {
                if climb_context_paced(&mut guard, made.used(), Instant::now()) {
                    guard.touch();
                }
            }
        },
        // A summary is not written with a command, so the words of a call are
        // passed by.
        &mut |_made: &[ToolCall]| {},
    );
    // Both hold the words the model gave. The longer of the two is the safer.
    let text = if answer.text.chars().count() >= said.chars().count() {
        answer.text.clone()
    } else {
        said
    };
    SummaryShot {
        text,
        finish: answer.finish.clone(),
        calls: answer.tool_calls.clone(),
        stopped: answer.stopped,
        error: answer.error.clone(),
    }
}

// Ask the model for the handoff summary of the chat that is open, and write it
// into the mark of the summarisation. After this the words before the mark are
// not sent to the model again: the first message of the reader, the summary, and
// the words that come after the mark are the whole of the talk as far as the
// model is concerned. The reader still sees all of the talk, and the line at the
// right of the chat keeps one node for every summarisation of it.
//
// The mark goes into the chat before the ask, for a server that goes away in the
// middle of a summary must leave a sign of it: the mark stands with nothing in
// it and the reason beside it, and the reader starts the summary again from the
// point it stood at.
//
// None of the writing is shown to the reader, and the model is not stopped at the
// limit of the summarisation while it writes: the summary is given all of the
// room that stands over after the words of the talk. When the room runs full
// under the first summary it is asked again for a short one, of the second prompt
// of the settings. True is given back when the summary was kept, which is the
// word that the room was made and the work may be gone on with.
fn run_handoff(
    state: &Arc<Mutex<State>>,
    server: &mut Option<Server>,
    cancel: &Arc<AtomicBool>,
) -> bool {
    // Take the order. An answer and a summary cannot both be on the way, for
    // both write into the same chat.
    let (model, settings, prompt, short, before, history, id) = {
        let Ok(mut guard) = state.lock() else {
            return false;
        };
        if guard.generating {
            return false;
        }
        let Some(model) = guard.model().clone() else {
            guard.set_status("Choose a model in the top bar first");
            return false;
        };
        if guard.chat().is_none() {
            return false;
        }
        let prompt = config::sanitize_handoff_prompt(&guard.cfg.chat_compact_prompt);
        let short = config::sanitize_handoff_prompt_short(&guard.cfg.chat_compact_prompt_short);
        let before = guard.ctx_used;
        // The talk as it stands is what the summary is written of. `build_history`
        // already leaves out the words that the newest written summary stands
        // for, and it sends no word of a mark that was never written, so the
        // ask is made of the first prompt, the summary, and the words that
        // stand after the newest written mark: the whole of what there is to
        // summarise now, and no summary of a summary.
        let history = guard.build_history();
        guard.generating = true;
        guard.busy = true;
        // The words of the summary are written out of sight of the reader.
        guard.summarising = true;
        // Both live buffers start empty, so the words of an older answer do not
        // stand under the mark of the summarisation.
        guard.live_text = String::new();
        guard.live_reasoning = String::new();
        guard.live_tools = Vec::new();
        guard.progress = 0;
        guard.progress_on = false;
        let id = guard.begin_summary();
        guard.set_status("The handoff summary is being written");
        let wants = guard.settings_for(&model);
        (model, wants, prompt, short, before, history, id)
    };
    // From here to the end of the ask the window is told that a summary is being
    // written, and it draws no live answer.
    let _writing = SummaryWriting {
        state: state.clone(),
    };

    // A server that went away is started again, as in an answer. The lock of the
    // state is not held while the model is being loaded.
    match ensure_server(state, server, &model, &settings, cancel) {
        Ok(total) => {
            let Ok(mut guard) = state.lock() else {
                return false;
            };
            guard.busy = false;
            guard.ctx_total = total;
        }
        Err(problem) => {
            let Ok(mut guard) = state.lock() else {
                return false;
            };
            guard.busy = false;
            guard.generating = false;
            // The mark keeps nothing of its own, and the reason stands beside it,
            // so the reader can start the summary again.
            guard.finish_summary(&id, "", before, 0, &problem);
            guard.set_status(
                "The handoff summary was not written. Start it again from the summarisation timeline",
            );
            return false;
        }
    }

    let base_url = server.as_ref().map(|s| s.url.clone()).unwrap_or_default();
    // The slot of this chat, with the talk of it laid in the slot again when
    // another chat stood there last. The ask of the summary names the slot, for
    // the talk above the ask is the talk the server worked through in it.
    let slot = server
        .as_ref()
        .and_then(|running| context_claim(state, &running.url, &running.model_id));
    // The talk as it stands, with the ask for the summary after it as a message
    // of the reader. The tools of the chat stand offered as they stand for an
    // answer, so that the front of this ask is the front of the talk word for
    // word and the server keeps every bit of work it has already done on it. A
    // model that answers with a call instead of with the summary has the call
    // cancelled and is asked again.
    let tools = summary_tools(state);
    let mut shot =
        ask_summary_for_handoff(state, &base_url, &history, &prompt, tools.as_ref(), slot, cancel);
    // A long talk can trip the server over one ask and not over the next, so a
    // fault that leaves no words at all is asked once more before the reader is
    // told. A stop of the reader's own is never asked again.
    if shot.error.is_some() && !shot.stopped && !cancel.load(Ordering::Relaxed) {
        tell(state, "The summary did not arrive. Asking once more");
        shot = ask_summary_for_handoff(state, &base_url, &history, &prompt, tools.as_ref(), slot, cancel);
    }
    let mut summary = clean_summary(&shot.text);
    // The first ask was made again when the room ran full under it, which is the
    // usual way the window is kept. One more guard stands here for the ask that
    // filled the window without the second ask being needed: a model that thinks
    // at length before it writes spends the room on its thoughts, and the summary
    // is gone before the first word of it stands. Such an ask is made again, this
    // time for a short one, for the room is still past the limit and a chat left
    // there is stopped again on its next message.
    let mut asked_short = false;
    if summary.trim().is_empty()
        && shot.error.is_none()
        && !shot.stopped
        && !cancel.load(Ordering::Relaxed)
    {
        tell(
            state,
            "The summary filled the context without a word of itself. Asking for a shorter one",
        );
        let again = ask_summary_for_handoff(state, &base_url, &history, &short, tools.as_ref(), slot, cancel);
        let cleaned = clean_summary(&again.text);
        if !cleaned.trim().is_empty() {
            shot = again;
            summary = cleaned;
            asked_short = true;
        }
    }
    // The room ran full under the first summary, so its last sentence stops in
    // the middle of itself. It is asked again, this time for a short one, which
    // has a far better chance of coming to an end in the room that is left. The
    // second ask carries the talk and nothing of the first answer, so it has all
    // of the room the first one had. The long summary is kept for all that it
    // stops short when the short one does not come at all.
    if summary_too_long(&shot) {
        tell(
            state,
            "The summary filled the context. Asking again for a shorter one",
        );
        if !asked_short {
            let again = ask_summary_for_handoff(state, &base_url, &history, &short, tools.as_ref(), slot, cancel);
            let cleaned = clean_summary(&again.text);
            if !cleaned.trim().is_empty() {
                shot = again;
                summary = cleaned;
            }
        }
    }

    let Ok(mut guard) = state.lock() else {
        return false;
    };
    guard.generating = false;
    guard.busy = false;
    finish_progress(&mut guard);
    // The summary is never shown, so the live buffers are emptied rather than
    // read. What the model gave stands in the mark of the summarisation alone.
    guard.live_text = String::new();
    guard.live_reasoning = String::new();
    // The room the summary itself takes, read from the words that stand in the
    // mark: the number the server gives of the words the model wrote holds the
    // thinking that is hidden with the rest of it, and it reads the summary
    // bigger than it is. The measure of the words is the number the reader reads
    // beside the heading of the summarisation.
    let after = estimate_tokens(&json!([{ "role": "user", "content": &summary }]));
    // What became of the ask. A summary that never came, that was stopped, or
    // whose server went away, is no summary at all: the mark is left empty, the
    // chat keeps every word it had, and the reader starts it again. A summary
    // that the room cut short, twice, is kept for all that, for the room was full
    // when it was asked for and a full chat cannot wait for a better one.
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
    // The words are kept only when the ask itself came off: no fault of the
    // stream, no stop of the reader's, and words that are not nothing. The line
    // above says what happened, and this says whether anything was won by it.
    let kept = shot.error.is_none() && !shot.stopped && !summary.trim().is_empty();
    if !kept {
        guard.finish_summary(&id, "", before, 0, &why);
        // The meter goes back to what the talk of itself costs. The ask that
        // faulted was carrying the whole of the window, and the words of the
        // summary that never came are not kept, so the number of that ask says
        // nothing of the chat that still stands there.
        let made = estimate_tokens(&guard.build_history());
        guard.ctx_used = made;
        if let Some(chat) = guard.chat_mut() {
            chat.meta.context_used = made;
        }
        guard.set_status("The handoff summary was not written. Start it again from the summarisation timeline");
        guard.persist();
        return false;
    }
    guard.finish_summary(&id, &summary, before, after, &why);
    // The room the talk takes now, read from the words that stand after the
    // summary: the first prompt, the summary, and the words that come after it.
    // The number the server gave with the ask is not the number of the talk that
    // stands now, for it holds the words the summary stands for, which are no
    // part of what goes back to the model, and it reads the talk bigger than it
    // is. A meter that kept the number of the ask would read the chat as full
    // the moment the room was made, and the answer would end under the block of
    // a summary that had just freed the room. The measure of the words is the
    // number the next ask is weighed against, and the first number the server
    // gives on the next ask takes the meter where it stands, as it always does.
    let made = estimate_tokens(&guard.build_history());
    guard.ctx_used = made;
    if let Some(chat) = guard.chat_mut() {
        chat.meta.context_used = made;
        chat.meta.updated = store::now_secs();
    }
    let count = guard.handoffs().len();
    guard.set_status(&format!(
        "Summarised #{} of the chat. The conversation now takes {}",
        count, made
    ));
    guard.persist();
    true
}

// Whether the room of the talk calls for a handoff summary now, and if it does,
// ask for one. The check is made when an answer is over, and once only behind a
// message: a summary that did not make enough room is not asked for twice, for
// the model would spend its room on summaries of summaries.
//
// The limit is weighed on the words the next ask really carries, and not on the
// whole of the talk as it stands. The words of a call that was cut short at the
// limit are the longest message of a chat that came to it in the middle of a
// call, for they hold what the model had written; and they are not words of the
// talk, for they are the program telling the model what it had been put to, and
// the model answers them with the call again. Weighed on the whole of the talk,
// a chat that is past the limit only because of that telling could never be got
// under it again: the next ask would be stopped on its first word, and the
// answer would end under the block of a call that never ran. The telling goes
// out bounded to the head of the call, and it is the talk without it that is
// weighed here, which is what a second summarisation would be made of.
// True when a summary was kept, which the worker reads as the room being made
// and the work being gone on with.
// What the limit is weighed on. The two callers of `maybe_compact` stand at two
// different points of the talk, and the room is read differently at each.
enum Weigh {
    // The room the server counted. The caller is mid-answer, at the moment an
    // answer was cut at the limit: the words that filled the room are the words
    // of the call that was cut, and they stand in the server's number and in no
    // message of the chat yet, for the telling of the call is written after the
    // room is made. Weighed on the talk as it stands, the room would read empty,
    // and a summary that is the whole point of the cut would never be asked for.
    Room,
    // The talk as it stands, with the telling of a cut call left out of it. The
    // caller is at the end of an answer, and the telling of a call that was cut
    // stands in the chat: it is the program speaking and not a word of the talk,
    // and a talk that is past the limit only because of it could never be got
    // under it again, for the summary ask that carries it would be stopped on
    // its first word.
    Talk,
}

fn maybe_compact(
    state: &Arc<Mutex<State>>,
    server: &mut Option<Server>,
    cancel: &Arc<AtomicBool>,
    weigh: Weigh,
) -> bool {
    let wanted = {
        let Ok(guard) = state.lock() else { return false };
        if guard.generating || guard.busy || guard.chat().is_none() {
            false
        } else if guard.handoff_owed() {
            // A summary is wanted and never came. The reader is asked to start it
            // again: the program does not put a model to work behind their back.
            false
        } else if guard.chat().map(ends_on_summary).unwrap_or(false) {
            // The newest thing of the chat is a summarisation already, so there
            // is nothing new that is worth a summary of its own.
            false
        } else {
            let used = match weigh {
                Weigh::Room => guard.ctx_used,
                Weigh::Talk => carrying_the_cut(&guard.build_history()),
            };
            config::compaction_wanted(&guard.cfg, used, guard.ctx_total)
        }
    };
    if wanted {
        return run_handoff(state, server, cancel);
    }
    false
}

// Make the room in the middle of an answer, and say whether it was made. The
// handoff summary is asked for while the answer is still on its way, which it
// cannot be left to do on its own: an answer and a summary both write into the
// same chat, so the answer stands aside while the summary is written, and is
// taken up again when the words are in the mark. Whether the room came to under
// the limit again is all the answer needs out of this, and is the answer this
// gives back.
fn make_room(
    state: &Arc<Mutex<State>>,
    server: &mut Option<Server>,
    cancel: &Arc<AtomicBool>,
) -> bool {
    {
        let Ok(mut guard) = state.lock() else {
            return false;
        };
        if guard.handoff_owed() {
            // A summary was asked for before and never came. The reader is asked
            // to start it again, as ever, for the program does not put a model to
            // work behind their back.
            return false;
        }
        // The answer is laid aside while the summary takes the chat.
        guard.generating = false;
        guard.busy = false;
    }
    let _ = maybe_compact(state, server, cancel, Weigh::Room);
    let Ok(guard) = state.lock() else { return false };
    !guard.handoff_owed() && !config::compaction_wanted(&guard.cfg, guard.ctx_used, guard.ctx_total)
}

// The most the model is asked to go on with its work after a handoff summary of
// one send of the chat box. Once is enough. An answer that filled the room again
// after the summary made for it is a talk the reader has to send a word to, and a
// model asked again and again on its own would spend the room on summaries of
// itself.
const CONTINUE_ROOF: usize = 1;

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
    let mut made = watched(calls);
    // The gauge leaves for a mark of a chapter too. Such a call gives the
    // reader no block, but the model wrote it after the words it was sent.
    let gauge = finish_progress(state);
    // A call that stands where it stood is not being written any more, so the
    // word that says so is kept. A call whose words grew is being written again,
    // and says so too. Without this a call that was marked as written while the
    // model thought would stand under the block of its own words for the rest of
    // the answer.
    for (n, one) in made.iter_mut().enumerate() {
        if let Some(before) = state.live_tools.get(n) {
            if !one.words_moved(before) {
                one.done = before.done;
            }
        }
    }
    let grew = made != state.live_tools;
    if grew {
        state.live_tools = made;
    }
    gauge || grew
}

impl LiveTool {
    // True when the words of this call are not the words of the call that stood
    // in its place. The name, the words, the reason and the thing it is pointed
    // at are looked at, and not the word that says whether the model is done
    // with it.
    fn words_moved(&self, before: &LiveTool) -> bool {
        self.name != before.name
            || self.command != before.command
            || self.purpose != before.purpose
            || self.target != before.target
    }
}

// The model has moved on from the calls it was writing: it thinks again, or it
// writes words of its own. The blocks of those calls are no longer being
// written, and say so, until the run of the tool stands in the chat in their
// place. True when a block changed, so the caller says that the state changed.
//
// This is the whole of the fault of a block that would not leave off saying it
// was being written. A model that thinks, writes a call, and then thinks again
// leaves the call finished long before the step of the answer comes to an end,
// and the block stood on 'writing' the whole while, with no word of a result.
fn end_writing_calls(state: &mut State) -> bool {
    let mut changed = false;
    for one in state.live_tools.iter_mut() {
        if !one.done {
            one.done = true;
            changed = true;
        }
    }
    changed
}

// The meter of the context while an answer comes in. The server says how many
// words the talk takes on every piece of the answer, so the meter moves during
// the answer and not only when the answer is over. The meter only climbs: the
// words of an answer that is still coming are not taken back. The number the
// server gives at the end of the answer is kept for the end, and that number
// may be smaller, for a stop takes words away. True when the number moved, so
// the caller says that the messages changed.
fn climb_context(state: &mut State, made: u64) -> bool {
    if made == 0 {
        return false;
    }
    // The first number of a new ask is the measure of the talk as the server has
    // it now, so it is taken where it stands. A number kept from an answer that
    // was stopped is not a measure of anything: the words it counted went out of
    // the talk with the answer.
    if state.ctx_fresh {
        state.ctx_fresh = false;
        if made == state.ctx_used {
            return false;
        }
        state.ctx_used = made;
        return true;
    }
    if made <= state.ctx_used {
        return false;
    }
    state.ctx_used = made;
    true
}

// How often the meter of the context may move while an answer comes in. The
// server speaks of the talk on every piece of it, which is a great deal oftener
// than a reader can read a number, and a number written again and again reads as
// a shake rather than as a fill. Half a second shows the talk filling often
// enough, and the number the server gives at the end of the answer stands
// whatever the pace, so nothing of it is lost.
const LEAST_CONTEXT_STEP: Duration = Duration::from_millis(500);

// The pace for the pieces that come in during an answer: the climb above, with
// the half second between the numbers. The first number of a new ask is taken
// where it stands, pace or no pace, for it is the measure of the talk as the
// server has it now, and a number kept from an answer that was stopped is not a
// measure of anything. `now` is the moment of the piece, so the rule can be
// tried without waiting for the half second. A number that is turned away is not
// kept back either: the next piece that comes after the half second brings the
// talk up to date at once.
fn climb_context_paced(state: &mut State, made: u64, now: Instant) -> bool {
    if !state.ctx_fresh {
        if let Some(begun) = state.ctx_at {
            if now.duration_since(begun) < LEAST_CONTEXT_STEP {
                return false;
            }
        }
    }
    let moved = climb_context(state, made);
    if moved {
        state.ctx_at = Some(now);
    }
    moved
}

// The number of the talk at the end of an answer. The number the server gives
// with the last piece is the room the words of that ask took; the talk holds more
// than it did when the ask went out, for the answer of the model and the runs of
// the tools it asked for were written into the chat on the way. The meter therefore
// keeps the larger of the two, which is the room the next ask takes.
//
// When no number came back -- the reader stopped the answer, or the server could
// not be reached -- the talk that stands is the measure. A stopped answer takes the
// words the model had written out of the chat again, so the live number of that
// answer is a number of a talk that no longer exists, and a meter that held it could
// not climb away from it.
fn used_after(usage: u64, history: &Value) -> u64 {
    usage.max(estimate_tokens(history))
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

// A silence worth asking over. The model came to its own end with nothing to
// read, no command asked for and nothing that went wrong: it thought, and then
// it stopped, which is not an end of the work it was given. A stop because the
// room for the context was used up is not that kind of silence, for asking again
// would only fill the same room again. How many times the silence was asked over
// already, and how many times it may be, decide the rest.
fn worth_a_silence(finish: &str, spent: usize, most: usize) -> bool {
    finish != "length" && spent < most
}

// Answer one prompt. When the model asks for a terminal command, the command
// runs and the model is asked again with the result. The files the reader
// hung on the message are written to disk first, and the message of the
// reader carries their marks, in the order the reader hung them.
//
// A turn that only goes on with the work adds no message of the reader: the
// newest handoff summary stands at the end of the talk already, and the words
// that name it tell the model to carry on from there. What the answer came to
// goes back to the worker, so it knows whether the work may be gone on with
// after the room has been made.
fn run_turn(
    state: &Arc<Mutex<State>>,
    server: &mut Option<Server>,
    cancel: &Arc<AtomicBool>,
    turn: Turn,
) -> Ended {
    let going_on = matches!(turn, Turn::Continue);
    // Whether this turn puts a message of the reader into the chat. A turn that
    // goes on after a handoff summary adds nothing, and a turn that writes an
    // answer again adds nothing either: the message the reader is going on from
    // stands in the talk already.
    let adding = matches!(turn, Turn::Sent(_));
    // A resumed answer is asked with the talk as it stands and with no tool
    // offered: a call cut in half cannot be mended, and the reader stopped the
    // turn to hear the rest of a saying, not to have a command begun.
    let resuming = matches!(turn, Turn::Resume(_));
    // The answer the reader stopped, when this turn goes on with one. The words
    // the model writes join onto it, rather than standing as an answer of their
    // own.
    let resumed = match &turn {
        Turn::Resume(id) => Some(id.clone()),
        _ => None,
    };
    let (text, files) = match turn {
        Turn::Sent(sent) => (sent.text, sent.files),
        Turn::Continue | Turn::Retry | Turn::Resume(_) => (String::new(), Vec::new()),
    };
    // A turn that only goes on adds no message of the reader to the chat, for the
    // words the model is asked with come out of the talk as it stands. A talk that
    // cannot be sent as it stands is given a word of its own, for the reason told
    // where `CONTINUE_PROMPT` is chosen below.
    // The files wait in the folder of the chat that carries them, so the
    // model can be asked again about them after a restart of the program, and
    // a command of the tool can read them in the room it works in.
    let shelf = state
        .lock()
        .ok()
        .and_then(|keeper| keeper.chat().map(|chat| keeper.attach_place_for(chat)));
    let kept: Vec<Attachment> = match &shelf {
        Some(place) => attach::save_at(place, &files),
        // No chat to carry them, so there is no room to keep them in.
        None => Vec::new(),
    };
    let lost = files.len().saturating_sub(kept.len());
    // Take the work order first, so two answers cannot mix.
    let (model, settings, workdir, sandbox_on, access) = {
        let Ok(mut state) = state.lock() else { return Ended::Halted };
        if state.generating {
            return Ended::Halted;
        }
        let Some(model) = state.model() else {
            state.set_status("Choose a model first");
            return Ended::Halted;
        };
        if state.chat_mut().is_none() {
            state.set_status("Start a new chat first");
            return Ended::Halted;
        }
        // What the chat asks for, and nothing else. Whether the jail really
        // works on this machine is looked at by the tool itself, which refuses
        // the run when the jail cannot be had. A chat that wants the sandbox is
        // never served a plain run because the jail is broken.
        let sandbox_on = state
            .chat()
            .map(|c| c.meta.sandbox_enabled)
            .unwrap_or(false);
        let access = state.access_of_open();
        let workdir = state
            .chat()
            .map(|c| state.workdir_for(&c.clone()))
            .unwrap_or_else(|| config::paths().data);
        // The question goes into the chat at once, with its attachments
        // hanging on it in the order the reader hung them. A turn that only goes
        // on with the work after a handoff summary adds nothing: the mark of the
        // summarisation is the last thing in the chat, and the model is asked
        // again from behind it.
        // A chat with nothing in it is a new one, and the side bar has to hear
        // that it came: the first message gives it its name and puts it in the
        // list of its place.
        let fresh = state
            .chat()
            .map(|c| c.messages.is_empty())
            .unwrap_or(false);
        if let Some(chat) = state.chat_mut() {
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
        if adding {
            // A message of the reader begins a turn of its own, so the mark of an
            // answer that never came to its end is no longer wanted: the reader has
            // written past it.
            state.clear_stopped_marks();
        }
        if lost > 0 {
            let text = format!(
                "{} attachments were too large, or could not be stored, so they were not sent",
                lost
            );
            state.push_note(&text, true);
        }
        // A picture the model cannot be shown is said out loud, so the reader
        // does not wonder why the model looks blind at it.
        let pictures = kept.iter().filter(|a| a.image).count();
        if pictures > 0 && !state.pictures_can_go() {
            state.push_note(
                "The images of this message cannot be shown to this model: turn vision on in the model settings, then send the message again",
                true,
            );
        }
        state.persist();
        if fresh {
            state.touch_structure();
        }
        state.generating = true;
        state.busy = true;
        // Both live buffers start empty, so a new turn never shows the
        // words of an older one, not even from another chat.
        state.live_text = String::new();
        state.live_reasoning = String::new();
        state.live_tools = Vec::new();
        // A question the model had put to the reader before this answer began is
        // gone with the answer that asked it: the popup that stood over the chat
        // box was asked by a run that is no more.
        state.asking = None;
        // The meter of the context is paced, and the pace begins anew with this
        // answer, so the first number of a new answer is stood on at once.
        state.ctx_at = None;
        state.set_status("The model is loading");
        let settings = state.settings_for(&model);
        (model, settings, workdir, sandbox_on, access)
    };

    // Make sure a server runs for this model. This can take a long time, and
    // the state lock is not held while it happens.
    let ready = ensure_server(state, server, &model, &settings, cancel);
    match ready {
        Ok(total) => {
            let Ok(mut state) = state.lock() else { return Ended::Halted };
            state.busy = false;
            state.ctx_total = total;
            state.set_status("Thinking");
        }
        Err(problem) => {
            let Ok(mut state) = state.lock() else { return Ended::Halted };
            state.busy = false;
            state.generating = false;
            state.push_note(&problem, true);
            state.set_status("The model did not start");
            return Ended::Halted;
        }
    }

    // The measure of the talk begins anew with this ask: the first number the
    // server gives is taken where it stands, so a number left over from an answer
    // that was stopped cannot hold the meter still. See `climb_context`.
    if let Ok(mut guard) = state.lock() {
        guard.ctx_fresh = true;
    }

    // The word the ask of a turn that only goes on carries with it, when the talk it
    // is given ends on a call: see `CONTINUE_PROMPT`. The talk is read as it stands
    // at this moment, for the runs of a tool that came out of the answer were written
    // into the chat after the ask was made ready, and it is the talk of the answer as
    // the runs left it that the next ask goes out with.
    // The word is looked for again before every step, and not once for the turn, for
    // an answer whose writing was stopped at the limit goes on with the telling of
    // the call it was stopped at at the end of the talk: see `CUT_CONTINUE_WORDS`.
    let mut carried: Option<String> = None;

    let base_url = server.as_ref().map(|s| s.url.clone()).unwrap_or_default();
    // The slot of this chat. The talk it stands on was worked through in that
    // slot, so every step of this answer is asked of the same slot and none of
    // the work is done twice. None when the keeping of the talks is switched
    // off, and the ask then goes out with no slot named, as it always did.
    let slot = server
        .as_ref()
        .and_then(|running| context_claim(state, &running.url, &running.model_id));
    // The two rooms of the Chatting settings, read once for this answer. None
    // means no ceiling, which is how both stand for a new set of settings: the
    // answer then ends when the model stops by itself, and a model may run as
    // many commands as it wants. A mark of a chapter eats neither room.
    let (command_room, answer_room) = {
        let Ok(guard) = state.lock() else { return Ended::Halted };
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
    // How many times the writing has been stopped at the limit set for the
    // summarisation in this answer, and the hand of the program that stops it.
    // The hand is put up by the piece of the answer that speaks of a talk past the
    // limit, and looked at by the stream after every piece.
    let mut cuts = 0usize;
    // How many times an answer of this one came back with nothing to read and was
    // asked over again. The room for that is read from the Chatting settings.
    let mut silences = 0usize;
    let silence_room = {
        let Ok(guard) = state.lock() else { return Ended::Halted };
        guard.cfg.chat_silent_retries.max(0) as usize
    };
    let room_cut = Arc::new(AtomicBool::new(false));
    let outcome = loop {
        steps += 1;
        // The hand is let go again for every step: the step that is going to come
        // now is to be written out, unless the talk comes to the limit again under
        // it.
        room_cut.store(false, Ordering::Relaxed);
        let cut_flag = room_cut.clone();
        // The word this step is asked with. A turn that only goes on with the work
        // after a handoff summary of the chat box is asked with the word that names
        // the call the talk ends on, and an answer that was stopped at the limit and
        // went on after the room was made is asked with the word that names the
        // telling of the call it was stopped at. Both stand for a call the server
        // would else owe an answer to.
        let carried_prompt: &str = match &carried {
            Some(word) => word.as_str(),
            None if going_on => CONTINUE_PROMPT,
            None => "",
        };
        let (history, code_on, search_on, chapters_on, switches) = {
            let Ok(guard) = state.lock() else { return Ended::Halted };
            (
                go_on_words(&guard, carried_prompt),
                guard.chat().map(|c| c.meta.code_enabled).unwrap_or(false),
                guard
                    .chat()
                    .map(|c| c.meta.search_enabled)
                    .unwrap_or(false),
                guard.cfg.chat_chapters,
                guard
                    .chat()
                    .map(|c| c.meta.tools.clone())
                    .unwrap_or_default(),
            )
        };
        // The tools of the servers of the reader's own. A server whose tools
        // were never read is looked at here, out of the lock, and once: what
        // the server answered is kept, so the next step and the next answer of
        // this chat ask the server nothing again until the reader changes it.
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
        // Any of the tools is enough to let the model ask for one. A resumed
        // answer is asked with none: the words the reader stopped stand at the end
        // of the talk, and a call cut in half cannot be mended.
        let use_tools = !resuming
            && (code_on || search_on || chapters_on || !mcp_tools.is_empty());
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
            if let Some(list) = offered(code_on, search_on, chapters_on, access, &mcp_tools, &switches) {
                body["tools"] = list;
                body["tool_choice"] = json!("auto");
            }
        }
        // The slot this chat stands in. The server takes the ask of a named
        // slot, waits while that slot is busy, and stands on the talk it
        // already worked through there; without the name it picks a slot by the
        // luck of the front of the words, and a chat that went away and came
        // back is read through again from its first word.
        if let Some(slot) = slot {
            body["id_slot"] = json!(slot);
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
            // Neither speed is known until the server says something of it, and
            // the clocks of the program have nothing to measure until a word of
            // the answer arrives.
            guard.prompt_rate = 0.0;
            guard.answer_rate = 0.0;
            guard.rate_start = None;
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
            &room_cut,
            &mut |piece: &str| {
                if let Ok(mut guard) = stream_state.lock() {
                    guard.live_text.push_str(piece);
                    // The first word is here, so the words that were sent are
                    // worked through and the gauge has nothing left to say.
                    if finish_progress(&mut guard) {
                        guard.touch();
                    }
                    // A server that says nothing of speed leaves the clocks to
                    // the program. The words of the answer are read as tokens of
                    // some four letters, which is how a token of talk is mostly
                    // made, and the clock is the one that ran from the first word
                    // to this one. The speed the server gives wins as soon as it
                    // comes, for it counts the tokens and not the letters.
                    if guard.answer_rate == 0.0 {
                        let began = guard.rate_start.unwrap_or_else(|| {
                            let now = Instant::now();
                            guard.rate_start = Some(now);
                            now
                        });
                        let secs = began.elapsed().as_secs_f64();
                        if secs > 0.5 {
                            guard.answer_rate =
                                (guard.live_text.chars().count() as f64 / 4.0) / secs;
                        }
                    }
                    // Words have come, so the thinking of this moment is over.
                    // It may begin again before the answer ends.
                    if guard.thought_start.is_some() {
                        guard.live_thought_done = true;
                    }
                    // Words of the answer have come, so the model has moved on
                    // from any call it was writing. The block of that call says
                    // so now, instead of going on saying it is being written.
                    if end_writing_calls(&mut guard) {
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
                    // The model thinks again, so whatever call it was writing is
                    // written: nothing more comes of those words, and the block
                    // of it must stop saying it is being written while the
                    // thinking goes on over it.
                    if end_writing_calls(&mut guard) {
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
                    // The speed of the prompt work comes from the same piece: the
                    // tokens done with over the clock they took. The gauge and the
                    // number of the speed stand and go together, so the speed is
                    // written whenever the gauge moved.
                    let speed = made.per_second();
                    if speed > 0.0 && (speed - guard.prompt_rate).abs() >= 0.05 {
                        guard.prompt_rate = speed;
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
                    if climb_context_paced(&mut guard, made.used(), Instant::now()) {
                        guard.touch();
                    }
                    // The same piece carries the two speeds of the work. The
                    // server is asked for the numbers of every piece, so the
                    // number of the speed moves while the model writes, and not
                    // only when it has done. A nought is passed over, for the
                    // first token of a run has no speed of its own yet.
                    let mut told = false;
                    if made.decode_per_second > 0.0
                        && (made.decode_per_second - guard.answer_rate).abs() >= 0.05
                    {
                        guard.answer_rate = made.decode_per_second;
                        told = true;
                    }
                    if made.prompt_per_second > 0.0
                        && (made.prompt_per_second - guard.prompt_rate).abs() >= 0.05
                    {
                        guard.prompt_rate = made.prompt_per_second;
                        told = true;
                    }
                    if told {
                        guard.touch();
                    }
                    // The limit of the summarisation is looked at here, with the
                    // same two numbers the meter of the context is made of, and
                    // the writing is stopped at once when the limit is come to. It
                    // cannot wait for the end of the answer, for a model writing a
                    // tool call bigger than the room it has never comes to an end
                    // of itself: it writes until the room is gone, which is the
                    // very thing the limit was set aside to prevent. What the
                    // model has written so far stands, and goes back to it.
                    if config::compaction_wanted(&guard.cfg, guard.ctx_used, guard.ctx_total) {
                        cut_flag.store(true, Ordering::Relaxed);
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

        let Ok(mut guard) = state.lock() else { return Ended::Halted };
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

        // The talk came to the limit set for the summarisation while the model was
        // still writing, so the writing was stopped there. Nothing else stops it:
        // a model with a tool call bigger than the room it has writes on until the
        // room is gone, and the room being gone is what the limit was set aside to
        // prevent. What the model wrote stands, the call it was writing is written
        // down for the model to read again, the room is made, and the answer is
        // asked on from where it was stopped.
        if answer.interrupted {
            // The call, or the calls, that were being written. A call stopped
            // before it had a name of its own is still a call the model was put to
            // and must hear of, so long as it has some words.
            let cut: Vec<ToolCall> = answer
                .tool_calls
                .iter()
                .filter(|call| is_writing(call))
                .cloned()
                .collect();
            // The words the model had given of its own answer stand in the chat, as
            // they would had the answer come to its end. The thinking of the turn
            // rides with the first thing the reader is shown, as it does with a
            // command.
            let mut thoughts = answer_thinking.clone();
            if !answer_text.trim().is_empty() {
                guard.push_message("Assistant", &answer_text, &thoughts, thought_ms, false);
                thoughts = String::new();
            } else if !thoughts.trim().is_empty() {
                // The turn gave no words of its own, so the thinking stands on its
                // own. It stands before the mark of the summarisation, for the mark
                // goes in after it when the room is made, and the thought of the turn
                // the summary is written of must not come to stand behind the heading
                // of the summarisation: the reader would see it go away and come back
                // on the far side of it.
                guard.push_message("Assistant", "", &thoughts, thought_ms, false);
                thoughts = String::new();
            }
            // The meter keeps the number the server last spoke of. An answer that
            // never came to its end gives no usage numbers of its own.
            let used_now = guard.ctx_used;
            // The meter of the context keeps the number the room climbed to, for it
            // only moves up. The room of the asks that follow is read again below,
            // out of the talk as it stands after the summarisation and the telling
            // of the call: the number of the ask that was cut holds the whole of the
            // room, and a meter that kept the larger of the two would read the chat
            // as full again the moment the room was made.
            if let Some(chat) = guard.chat_mut() {
                chat.meta.context_used = used_now;
                chat.meta.updated = store::now_secs();
            }
            // The reader is told before the room is made, for the making of it puts
            // the heading of a summarisation in the chat and the two belong one
            // under the other.
            let told = if cut.is_empty() {
                String::from(
                    "The conversation reached the summarisation limit while the answer was being written, so the writing stopped. Space is being made, and the model continues the answer",
                )
            } else {
                String::from(
                    "The conversation reached the summarisation limit while a tool call was being written, so the writing stopped and the call was cancelled. Space is being made, and the model is asked to write the call again",
                )
            };
            guard.push_note(&told, false);
            guard.persist();
            drop(guard);
            cuts += 1;
            // Make the room. The handoff summary is asked for here, in the middle
            // of an answer, for the answer can go no further without it and the
            // reader is not to be made to start a summary by hand for a fault of
            // the room.
            let room = make_room(state, server, cancel);
            let Ok(mut guard) = state.lock() else { return Ended::Halted };
            // The room the next ask takes is read from the talk as it stands now,
            // and not kept from the ask that was cut. That ask ended at the limit,
            // so its number holds the whole of the room, and a meter that kept the
            // larger of the two would read the chat as full again the moment the
            // room was made. The first number the server gives on the next ask
            // takes the meter where it stands, as it always does.
            let made = estimate_tokens(&guard.build_history());
            guard.ctx_used = made;
            // How much of the words of the stopped call the room that was made can
            // carry. A call whose whole of words were sent back would fill the
            // room again, and the model would be stopped before it had a word.
            let bound = cut_bound(guard.ctx_used, guard.ctx_total);
            // The calls that were stopped are written down after the summarisation
            // and never before it, for the words that stand before a summarisation
            // go back to the model no more. The model has to be told what it was
            // put to, and the telling has to stand where the model will read it.
            for call in cut.iter() {
                guard.push_cancelled(call, &thoughts, thought_ms, bound);
                thoughts = String::new();
            }
            if !thoughts.trim().is_empty() {
                // Nothing of the turn was shown to the reader, so its thinking
                // stands on its own rather than being lost.
                guard.push_message("Assistant", "", &thoughts, thought_ms, false);
            }
            // The meter of the talk is written to the chat, for the reader reads
            // it from there, and the telling of a stopped call costs room as much
            // as anything else does. The number the server gave at the cut is not
            // kept, for it holds the whole of the room.
            let used_now = guard.ctx_used;
            if let Some(chat) = guard.chat_mut() {
                chat.meta.context_used = used_now;
            }
            guard.persist();
            // Whether the room that was made is past the limit is weighed on the
            // words the next step of the answer really carries, and not on the
            // whole of the talk as it stands. The telling of the call that was
            // stopped is the program speaking and not a word of the talk: it is
            // the longest message of a chat that came to the limit in the middle
            // of a call, for it holds what the model had written. Weighed on the
            // whole of the talk, a chat that is past the limit only because of
            // that telling could never be got under it again -- the answer ended
            // under the block of a call that never ran, and every fresh send ended
            // the same way. So the room is weighed with the telling left out,
            // which is what a second summarisation is made of, and which is what
            // the model is asked with once the words of the call are bounded to
            // the room. The weighing goes by the same measure as the limit was
            // come to, and it says nothing of what the server will count when the
            // ask goes out: the meter is read again on the first number of the
            // next ask, and the hand is put up again on the word that is past the
            // limit. A summary that was kept is room that was made, and the answer
            // goes on after it whatever the estimate says, for as long as each
            // summarisation frees the room the answer goes on after it: a task
            // that fills the room again and again is summarised again and again,
            // and the work is not begun over for the fault of the room. The
            // answer ends only when the room that was made is still past the
            // limit: the model is writing something no room will hold, and the
            // reader has to hear so rather than watch the same room be made and
            // spent for ever.
            let room_asked = carrying_the_cut(&guard.build_history());
            let still_full = config::compaction_wanted(&guard.cfg, room_asked, guard.ctx_total);
            if !room || (still_full && cuts > 1) {
                // Either the summary never came, which the reader is asked of in
                // the usual way, or the room is still past the limit even with the
                // telling of the stopped call left out of it. Either way the answer
                // ends here, rather than be stopped again at once on the first word
                // of the next ask.
                guard.generating = false;
                let text = if guard.handoff_owed() {
                    "The handoff summary was not written. Start it again from the summarisation timeline"
                } else if still_full {
                    "The context is still over the summarisation limit, so the answer ends here. Ask for the work in parts small enough for the context, or give the model a larger context"
                } else {
                    "The summarisation made no space, so the answer ends here"
                };
                guard.set_status(text);
                break Ended::Halted;
            }
            // The talk that the words of the stopped call went into ends on the
            // telling of them, which is a call the server is owed an answer to: the
            // telling asks for the call again, and nothing of the reader stands after
            // it. A word of the program therefore goes out behind it, so the ask ends
            // on a question. Without it a server set to read calls out of an answer
            // writes one of its own, and the words of it are thrown away with the ask
            // that made it, so the step that follows the room being made gives back
            // no answer at all and the chat stands under the block of the call that
            // never ran. See `cut_asks_for_a_call`. The word is no note of the chat,
            // for the reader never wrote it and has nothing to read where it stands;
            // it rides on the ask alone, as `CONTINUE_PROMPT` does. The first message
            // of the reader keeps the front of the prompt, so it costs the room of a
            // fresh tail and nothing else.
            if cut_asks_for_a_call(&guard.build_history()) {
                carried = Some(String::from(CUT_CONTINUE_WORDS));
            }
            // The room is made and the telling of the call stands where the model
            // will read it, so the answer is asked on from there. It is not gone on
            // with by the worker, for the worker asks the model to go on once for a
            // send, and that one going on is spent on the summarisation of the chat
            // box. The step that comes now is asked afresh out of the talk as it
            // stands, so the work is not begun again.
            guard.generating = true;
            guard.set_status("Space was made. The model is continuing the answer");
            continue;
        }

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
        // The calls the reader stopped with their own hand while the model was
        // still writing them. A mark of a chapter is not one of them: it runs
        // nothing, the reader is never shown a block of it, and a block for it
        // would read as a command that never ran.
        let stopped_runs: Vec<ToolCall> = if answer.stopped && use_tools {
            let writing: Vec<ToolCall> = answer
                .tool_calls
                .iter()
                .filter(|call| is_writing(call))
                .cloned()
                .collect();
            split_calls(writing).1
        } else {
            Vec::new()
        };
        // A turn that stopped in the middle of a call has a block the reader is
        // shown, so its thinking stands with that block, and no line is written
        // about an answer that was never meant to come.
        let stopped_call = !stopped_runs.is_empty();
        let turn_thinking = if no_words && (asked_for_tool || stopped_call) {
            answer_thinking.as_str()
        } else {
            ""
        };
        // A turn stopped in the middle of a call asked for something, and the
        // block of that call is what the reader is shown of it: a turn that asked
        // for a tool writes no line of its own about a missing answer.
        match kept_of(
            &answer_text,
            &answer_thinking,
            answer.stopped,
            asked_for_tool || stopped_call,
            answer.error.is_some(),
        ) {
            // The words are kept as they came in, also when the reader
            // stopped the run.
            Kept::Words => {
                // An answer that was gone on from joins the answer it was gone on
                // from, so the reader reads one answer and not two.
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
                    guard.push_message(
                        "Assistant",
                        &answer_text,
                        &answer_thinking,
                        thought_ms,
                        false,
                    );
                }
            }
            Kept::Thinking => {
                guard.push_message("Assistant", "", &answer_thinking, thought_ms, false)
            }
            // The model used tokens but left nothing to read, and asked for no
            // command. This happens when the context ran full, and when the model
            // came to its own end straight out of a thought.
            Kept::Note => {
                // A silence of the second kind is asked over, rather than written
                // off as an answer at once. The thought the model made stands in
                // the talk, so it reads its own thinking again and goes on from
                // there, and the reader is not left under a line about an answer
                // that never came while the work was still before it.
                if worth_a_silence(&answer.finish, silences, silence_room)
                    && !cancel.load(Ordering::Relaxed)
                {
                    silences += 1;
                    if !answer_thinking.trim().is_empty() {
                        guard.push_message("Assistant", "", &answer_thinking, thought_ms, false);
                    }
                    guard.persist();
                    // The step the silent ask took is given back: a model that said
                    // nothing spent none of the room for steps.
                    steps = steps.saturating_sub(1);
                    guard.set_status("The model said nothing. It is asked again");
                    continue;
                }
                let note = if answer.finish == "length" {
                    String::from("(the context was full before the model could answer)")
                } else if silences > 0 {
                    format!("(the model gave no answer, although it was asked {} times)", silences + 1)
                } else {
                    String::from("(the model gave no answer)")
                };
                guard.push_message("Assistant", &note, &answer_thinking, thought_ms, false);
            }
            Kept::Nothing => {}
        }

        // The calls the reader stopped stand in the chat. The tool never ran, and
        // the block of the words being written goes with the answer instead of
        // vanishing with it: the reader stopped a thing they could see being
        // written, and it has to stay where they can still read it. Each call goes
        // back to the model as the words of a turn, as a call the room gave out
        // under does, for its words are not whole.
        let mut stopped_thoughts = turn_thinking.to_string();
        for call in stopped_runs.iter() {
            // The thinking of the turn stands with the first block, as it does
            // with a run.
            let thinking = std::mem::take(&mut stopped_thoughts);
            guard.push_stopped_by_reader(call, &thinking, thought_ms, GREATEST_CUT_ARGS);
        }

        // Tokens for the context meter. The number the server gave at the end of
        // this ask is the room the words of that ask took, and the talk holds more
        // than it did when the ask went out: the answer of the model, and the run
        // of the tool it asked for. The meter therefore keeps the larger of the
        // two, the room the ask took and what the talk of itself costs now, which
        // is the room the next ask will take. An answer the reader stopped gives
        // no number at all, and the words it had written went out of the talk with
        // it, so there the talk that stands is the measure.
        let used = answer.prompt_tokens + answer.completion_tokens;
        let told = guard.build_history();
        guard.ctx_used = used_after(used, &told);
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
            // An answer the reader stopped never came to an end of its own, so it
            // carries the mark of the stop, and the page puts the mark of going on
            // with it under that answer alone. A stop that came at a call keeps the
            // block of the call instead, for a call cut in half cannot be mended;
            // an answer that came to its end leaves nothing to go on with, and the
            // mark of an older one goes away.
            if answer.stopped && stopped_runs.is_empty() {
                guard.mark_last_answer_stopped();
            } else if !answer.stopped {
                guard.clear_stopped_marks();
            }
            let text = if answer.stopped {
                "Stopped"
            } else if !finished && !nothing_left {
                if no_room_left {
                    "The answer stopped at the step limit in the Chatting settings"
                } else {
                    "The model asked for more steps than one answer allows"
                }
            } else {
                "Ready"
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
            // The worker is told what the answer came to. One that stopped on the
            // hand of the reader, or on a fault of the stream, is not gone on
            // with; one that came to its end, or to the ceiling of its steps, is.
            break if finished {
                Ended::Halted
            } else {
                Ended::Done
            };
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
            // Only a command of the terminal tool eats the room for commands.
            // The other tools work in the room of the chat in the program
            // itself, and the setting that holds the room speaks of commands.
            let terminal = call.name == tools::TOOL_NAME;
            // A tool the reader took out of the drop-down of the badge is never
            // offered, but a model that names it anyway is answered rather than
            // run: the badge of the tools is the door over all of them, and a
            // call of the model cannot open it.
            if !tool_allowed(state, &call.name) {
                let Ok(mut guard) = state.lock() else {
                    continue;
                };
                let note = format!(
                    "{} are switched off for this chat, so nothing ran",
                    tool_group_word(group_of(&call.name).unwrap_or("tools"))
                );
                let id = guard.begin_tool(call, &note, "", 0);
                guard.finish_tool(&id, &note, false);
                continue;
            }
            // The room for commands is spent. The call is still answered, so
            // the model hears why and can go on with its talk instead of
            // asking again for a command that never runs.
            if terminal && command_room.is_some_and(|room| commands >= room) {
                let Ok(mut guard) = state.lock() else {
                    continue;
                };
                let note = "The command limit for this answer is reached, so the command did not run";
                let id = guard.begin_tool(call, note, "", 0);
                guard.finish_tool(&id, note, false);
                continue;
            }
            if terminal {
                commands += 1;
            }
            // The first call the reader is shown takes the thoughts of the turn,
            // and no other one does.
            let thinking = std::mem::take(&mut thoughts);
            if terminal {
                run_command(call, &thinking, thought_ms, state, &workdir, sandbox_on, access);
            } else {
                run_program_tool(call, &thinking, thought_ms, state, &workdir, cancel);
            }
        }
        if !thoughts.is_empty() {
            // The turn asked for marks and for nothing a reader could look at,
            // so the thoughts stand above the marks as a panel of their own.
            let Ok(mut guard) = state.lock() else { return Ended::Halted };
            guard.push_message("Assistant", "", &thoughts, thought_ms, false);
        }
        let Ok(mut guard) = state.lock() else { return Ended::Halted };
        guard.set_status("The model is reading the tool result");
    };
    outcome
}

// True of a call the model had begun to write: it has a name of its own, or
// words of its arguments, or both. A call stopped before either came is nothing
// the reader was shown, and nothing to tell the model about.
fn is_writing(call: &ToolCall) -> bool {
    !call.name.trim().is_empty() || !call.arguments.trim().is_empty()
}

// The tools the model is offered for one step, or nothing at all when it is
// offered none. The terminal tool and the tools that work on the files stand and
// fall together with the badge of the tools, for all of them change what is on
// the machine. The web search stands on a switch of its own, for it reads the web
// out of the program and never the machine. The mark of a chapter stands on a
// switch of its own as well, for it runs nothing.
// `access` is what the chat lets a command reach, so the words of the terminal
// tool tell the model what it really has.
// The line of the drop-down of the badge of the tools that a tool of the program
// stands under. A tool outside the badge - the mark of a chapter, and every tool
// of a server of the reader's own - gives nothing back, for the badge holds no
// switch over it.
fn group_of(name: &str) -> Option<&'static str> {
    match name {
        tools::TOOL_NAME => Some("commands"),
        editfile::EDIT_NAME | editfile::CREATE_NAME => Some("files"),
        viewimage::NAME => Some("picture"),
        download::NAME => Some("hand_out"),
        askuser::NAME => Some("ask"),
        _ => None,
    }
}

// True when the open chat lets a call of this name through. The badge of the
// tools is read where the tools are offered; this is the same rule laid over the
// call itself, for a model can name a tool it was never offered.
fn tool_allowed(state: &Arc<Mutex<State>>, name: &str) -> bool {
    let Some(part) = group_of(name) else { return true };
    let Ok(guard) = state.lock() else { return false };
    guard
        .chat()
        .map(|c| c.meta.tools.value_of(part))
        .unwrap_or(true)
}

// The words of one line of the drop-down, for the line of the state and for the
// block of a call that was not let through.
fn tool_group_word(part: &str) -> String {
    match part {
        "commands" => String::from("Terminal commands"),
        "files" => String::from("File management"),
        "picture" => String::from("The look at a picture"),
        "hand_out" => String::from("Handing a file to the reader"),
        "ask" => String::from("Asking the reader"),
        other => String::from(other),
    }
}

fn offered(
    code_on: bool,
    search_on: bool,
    chapters_on: bool,
    access: tools::Access,
    mcp_tools: &[Value],
    switches: &store::ToolSwitches,
) -> Option<Value> {
    if !(code_on || search_on || chapters_on || !mcp_tools.is_empty()) {
        return None;
    }
    let mut list: Vec<Value> = Vec::new();
    // The badge of the tools is the door over all of them, and the lines of its
    // drop-down stand behind that door: a tool whose line is off is not offered,
    // and a badge that is off offers none of them whatever the lines say.
    if code_on {
        if switches.commands {
            list.push(tools::definition(access));
        }
        if switches.files {
            list.push(editfile::edit_definition());
            list.push(editfile::create_definition());
        }
        if switches.picture {
            list.push(viewimage::definition());
        }
        if switches.hand_out {
            list.push(download::definition());
        }
        if switches.ask {
            list.push(askuser::definition());
        }
    }
    if search_on {
        list.push(search::definition());
    }
    if chapters_on {
        list.push(tools::chapter_definition());
    }
    // The tools of the servers of the reader's own stand last, each under the
    // name of the server it came from.
    list.extend(mcp_tools.iter().cloned());
    // Nothing was left to offer. A chat with every line of the drop-down of the
    // badge switched off comes to this, and the call of the model must carry no
    // tools at all rather than an empty list of them.
    if list.is_empty() {
        return None;
    }
    Some(Value::Array(list))
}

// The wait for a word from the reader. The run asks the state what the reader
// answered, and rests between the asks, so the window is never kept from drawing
// and the reader is never hurried. A question of the ordinary kind is given up
// when its wait is gone; any question is given up with the answer when the reader
// stops the run, and a question the model cannot go on without is waited for as
// long as it takes.
fn wait_for_answer(
    state: &Arc<Mutex<State>>,
    id: &str,
    seconds: u32,
    cancel: &Arc<AtomicBool>,
) -> Option<String> {
    let began = Instant::now();
    loop {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        let held = state
            .lock()
            .ok()
            .and_then(|guard| guard.asking_answer_of(id));
        if held.is_some() {
            return held;
        }
        if seconds > 0 && began.elapsed().as_secs() >= seconds as u64 {
            return None;
        }
        std::thread::sleep(Duration::from_millis(150));
    }
}

// One run of the terminal tool. The block stands in the chat before the command
// starts, and says what became of it when it is over.
fn run_command(
    call: &ToolCall,
    thinking: &str,
    thought_ms: u64,
    state: &Arc<Mutex<State>>,
    workdir: &Path,
    sandbox_on: bool,
    access: tools::Access,
) {
    let (command, purpose) = tools::parse_arguments(&call.arguments);
    // The reason the model gave is not asked for any more, but a model that
    // gives one of its own is still shown.
    let shown = if purpose.trim().is_empty() {
        command.clone()
    } else {
        format!("{}\nreason: {}", command, purpose)
    };
    let message_id = {
        let Ok(mut guard) = state.lock() else { return };
        let id = guard.begin_tool(call, &shown, thinking, thought_ms);
        let text = format!("Running a command: {}", first_line(&command));
        guard.set_status(&text);
        id
    };
    let outcome = tools::run(&command, workdir, sandbox_on, access, TOOL_TIMEOUT);
    let mut output = outcome.summary();
    // The chat asked for the sandbox, so say when it was not used. The run
    // itself already says why, and it did not run at all then.
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
    guard.finish_tool(&message_id, &output, outcome.ok);
}

// One call of a tool the program does itself. None of these starts a shell, so
// none of them needs the jail: a chat whose jail cannot be had has them still.
// The file tools reach the room of the chat and nothing past it, which is the
// one room the jail may be written in.
fn run_program_tool(
    call: &ToolCall,
    thinking: &str,
    thought_ms: u64,
    state: &Arc<Mutex<State>>,
    workdir: &Path,
    // The hand the reader puts up to stop the answer. A question the reader was
    // asked is given up with the answer, for the run that asked it is going away.
    cancel: &Arc<AtomicBool>,
) {
    // The words of the call, and the words of the state line, are settled first:
    // the block has to stand in the chat before the work begins.
    let (shown, _, target) = words_of(&call.name, &call.arguments);
    // The head of the block names the file a file tool works on, and the
    // one thing another tool names when it has no file of its own.
    let shown = if target.is_empty() { shown } else { target };
    let doing = match call.name.as_str() {
        name if mcp::is_tool(name) => "A tool of a server",
        editfile::EDIT_NAME => "Editing a file",
        editfile::CREATE_NAME => "Creating a file",
        search::NAME => "Searching the web",
        viewimage::NAME => "Looking at an image",
        download::NAME => "Offering a file",
        askuser::NAME => "Asking the reader",
        _ => "A tool of the program",
    };
    let message_id = {
        let Ok(mut guard) = state.lock() else { return };
        let id = guard.begin_tool(call, &shown, thinking, thought_ms);
        let text = format!("{}: {}", doing, first_line(&shown));
        guard.set_status(&text);
        id
    };
    // A tool of a server of the reader's own. The server is named by the part
    // of the name of the tool that stands for it, and it is reached out of the
    // program: never inside the jail of a command, for a command has no network
    // and no program of its own to run.
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
            Some((server, tool)) => mcp::run(&server, &tool, &call.arguments),
            None => mcp::Report {
                ok: false,
                output: String::from(
                    "No server of this program is known by that name, so nothing ran",
                ),
            },
        };
        let Ok(mut guard) = state.lock() else { return };
        guard.finish_tool(&message_id, &report.output, report.ok);
        return;
    }
    // A picture is read here and kept out of the words the reader sees. The
    // receipt of the run is the block in the chat; the picture rides on the same
    // message as an attachment of the chat, and goes to the model with the words
    // of the receipt when the history is made again.
    if call.name == viewimage::NAME {
        let vision = {
            let Ok(guard) = state.lock() else { return };
            guard.pictures_can_go()
        };
        let made = viewimage::open(workdir, &call.arguments, vision);
        let records = match &made.picture {
            Some(picture) => {
                let Ok(guard) = state.lock() else { return };
                match guard.attach_place() {
                    Some(place) => attach::save_at(
                        &place,
                        &[attach::Incoming {
                            name: picture.name.clone(),
                            mime: picture.mime.clone(),
                            data: picture.data.clone(),
                            text: String::new(),
                        }],
                    ),
                    None => Vec::new(),
                }
            }
            None => Vec::new(),
        };
        let Ok(mut guard) = state.lock() else { return };
        guard.finish_tool_with(&message_id, &made.report.output, made.report.ok, records);
        return;
    }
    // A question the model puts to the reader. The run waits: the ask stands above
    // the chat box, the reader chooses one of the answers the model offered or
    // writes an answer of their own, and what they said goes back to the model as
    // the result of the call. A question of the ordinary kind is given up when its
    // wait is gone; a question the model cannot go on without is waited for until
    // it is answered, or until the reader stops the answer whole.
    if call.name == askuser::NAME {
        let asked = askuser::parse(&call.arguments);
        {
            let Ok(mut guard) = state.lock() else { return };
            let text = format!("Waiting for your answer: {}", first_line(&asked.text));
            guard.set_status(&text);
        }
        let (asking, seconds) = {
            let Ok(mut guard) = state.lock() else { return };
            guard.begin_asking(&asked)
        };
        let answer = wait_for_answer(state, &asking, seconds, cancel);
        let (words, ok) = match answer {
            Some(said) => (askuser::answered_words(&said), true),
            // A question that was given up is no fault of the run: the model is
            // told the reader did not answer, and goes on as well as it can.
            None if seconds > 0 => (askuser::given_up_words(seconds), true),
            None => (askuser::stopped_words(), false),
        };
        let Ok(mut guard) = state.lock() else { return };
        guard.finish_tool(&message_id, &words, ok);
        guard.clear_asking(&asking);
        return;
    }
    // A file the reader is offered. Nothing is copied and nothing is fetched: the
    // file stands in the room of the chat where the model put it, and the offer
    // rides on the message of the run. The page draws the blocks of the offer at
    // the end of the answer that made them, and only when the model has stopped
    // writing, so a file that is still being written is never handed over.
    if call.name == download::NAME {
        let made = download::offer(workdir, &call.arguments);
        let Ok(mut guard) = state.lock() else { return };
        guard.finish_gifts(&message_id, &made.report.output, made.report.ok, made.gifts);
        return;
    }
    let report = match call.name.as_str() {
        editfile::EDIT_NAME => editfile::edit(workdir, &call.arguments),
        editfile::CREATE_NAME => editfile::create(workdir, &call.arguments),
        // The web search needs no room of the chat: it reads what stands on the
        // web, and nothing of the machine.
        search::NAME => search::run(&call.arguments),
        other => tools::Report {
            ok: false,
            output: format!(
                "The model asked for {}, which this program does not offer",
                first_line(other)
            ),
        },
    };
    let Ok(mut guard) = state.lock() else { return };
    guard.finish_tool(&message_id, &report.output, report.ok);
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
    let (binary, gpus, wants, keeping) = {
        let Ok(state) = state.lock() else {
            return Err("The program state is gone".to_string());
        };
        let binary = state
            .server_binary()
            .ok_or("llama-server was not found in the folder from the settings")?;
        let wants = state.server_wants();
        // The folder the talks the model works its way through are written into,
        // when the reader has the keeping on. The server is told the folder and
        // nothing else: it makes the names of the files inside it by itself,
        // and never looks outside it. See `context_claim`.
        let keeping = state.cfg.chat_context_files.then(|| config::paths().contexts);
        (binary, state.gpus.clone(), wants, keeping)
    };
    let (thinking_mode, preserve, tools_wanted) =
        (wants.thinking_mode.clone(), wants.preserve, wants.tools);
    // The model loads once and stays loaded between the chats that ask the
    // same things of the server: see `ServerWants::covers` for the whole rule.
    let reusable = server.as_mut().map(|s| s.alive()).unwrap_or(false)
        && server
            .as_ref()
            .map(|s| ServerWants::covers(&State::server_wears(s), &wants))
            .unwrap_or(false);
    if reusable {
        let total = server
            .as_ref()
            .map(|s| s.ctx_total)
            .unwrap_or(settings.ctx_size as u64);
        return Ok(total);
    }
    stop_server(state, server);
    if cancel.load(Ordering::Relaxed) {
        return Err("Stopped before the model loaded".to_string());
    }
    match llama::Server::start(
        &binary,
        model,
        settings,
        &gpus,
        tools_wanted,
        &thinking_mode,
        preserve,
        keeping.as_deref(),
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
            stop_server(state, server);
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

// The name of the archive the reader is given when they take the whole of an
// offer at once. The first file of the offer gives its name to the archive, so
// the file that comes down is not a mystery; an offer with nothing to take its
// name from is called for the program.
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

    // A line of the drop-down of the badge of the tools takes its kind of tool out
    // of the chat, and the badge itself takes every one of them out with it. The
    // choice stands with the chat, and opens the next chat the same way.
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
            offered(code_on, false, false, tools::Access::default(), &[], switches)
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
        // Every line of the drop-down off, and the badge on: nothing of the
        // program is offered at all. The lines are read from the switches of the
        // chat themselves, so a tool the program learns later is taken out here
        // as well rather than leaving this test to say a thing that is no longer
        // true of it.
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
        assert!(words(true, &engine.snapshot().tool_switches).is_none());
        // The badge is the door over all of them: with the badge off, a line that
        // stands on offers nothing.
        engine.set_tool_switch("commands", true);
        engine.set_code(false);
        assert!(words(false, &engine.snapshot().tool_switches).is_none());
        // The next chat opens with the same lines, as the reader left them.
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
        assert!(kept.commands, "the line the reader put back on stays on");
        assert!(!kept.files, "file management is still off");
        assert!(!kept.picture);
        assert!(!kept.hand_out, "the offer of a file is still off");
        assert!(!lines.is_empty(), "the badge has lines to switch");
    }

    // The reader works in one chat with the tools on and a level of thinking
    // picked, and opens a new chat directly after. The new chat stands where
    // the one before it stood, so the server it asks for is the server that
    // already runs, and the model on it stays loaded.
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
            // A word of the talk, so the chat before is a real one.
            state.chat_mut().unwrap().messages.push(Message {
                role: String::from("You"),
                body: String::from("test"),
                ..Default::default()
            });
            state.server_wants()
        };
        assert_eq!(before.thinking_mode, "medium");
        assert!(before.tools);
        // `Create new chat` goes back to the first screen, and the first
        // message of it starts the chat. No model is picked here, so the message
        // stops before the worker; the chat is started the same way it is when
        // the message goes out.
        engine.new_chat();
        let mut state = engine.state.lock().unwrap();
        state.start_chat();
        let chat = state.chat().expect("the new chat stands open");
        assert_eq!(chat.meta.thinking_mode, "medium");
        assert!(chat.meta.code_enabled);
        assert!(chat.meta.search_enabled);
        assert!(!chat.meta.sandbox_enabled);
        assert!(chat.meta.preserve_thinking);
        // The new chat asks for the same server the chat before it ran on, so
        // the worker keeps it and the model on it.
        assert_eq!(state.server_wants(), before);
    }

    // A mode the chat wears already is not a change: the badge says the same
    // thing again, and the running server keeps its flags, its model and its
    // loaded words with it. Only a mode that moved sends the server away.
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
        // The same mode again: the chat stands where it stood, and the choice
        // for the next chat stands there too, so the new chat asks for the
        // server that is already up.
        engine.set_thinking("medium");
        let state = engine.state.lock().unwrap();
        assert_eq!(state.thinking_of_open(), "medium");
        assert_eq!(state.new_thinking, "medium");
    }

    // The whole rule of keeping a server, without a process behind it. The two
    // thinking flags move the server; the tool flag only has to cover the chat,
    // so a chat with the tools off sits on a server that was started with them
    // and pays no load for it.
    #[test]
    fn only_the_thinking_flags_and_a_tool_that_came_move_the_server() {
        let running = ServerWants {
            model_id: String::from("one"),
            thinking_mode: String::from("medium"),
            preserve: None,
            tools: true,
        };
        let wants = ServerWants {
            model_id: String::from("one"),
            thinking_mode: String::from("medium"),
            preserve: None,
            tools: true,
        };
        // The chat the server was started for sits on it, of course.
        assert!(ServerWants::covers(&running, &wants));
        // A chat that wants none of the tools sits on this server as it is.
        let quiet = ServerWants { tools: false, ..wants.clone() };
        assert!(ServerWants::covers(&running, &quiet));
        // A chat that wants tools does not sit on a server started without
        // them, for such a server reads no tool call.
        let bare = ServerWants { tools: false, ..running.clone() };
        assert!(!ServerWants::covers(&bare, &wants));
        // A chat that wants none of them does, and pays no load for it.
        assert!(ServerWants::covers(&bare, &quiet));
        // The two thinking flags move the server, and so does another model.
        let other = ServerWants {
            thinking_mode: String::from("low"),
            ..wants.clone()
        };
        assert!(!ServerWants::covers(&running, &other));
        let kept_on = ServerWants {
            preserve: Some(true),
            ..wants.clone()
        };
        assert!(!ServerWants::covers(&running, &kept_on));
        let second = ServerWants {
            model_id: String::from("two"),
            ..running.clone()
        };
        assert!(!ServerWants::covers(&second, &wants));
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

    // A chat that was started and never written in leaves no file, so the files
    // hold only the chats that hold something.
    #[test]
    fn a_chat_is_written_when_it_holds_a_message() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        let chats = config::paths().chats;
        let stored = || {
            std::fs::read_dir(&chats)
                .map(|entries| entries.count())
                .unwrap_or(0)
        };

        // A run starts on the first screen, with no chat and nothing on disk.
        assert!(engine.snapshot().home);
        assert_eq!(stored(), 0);

        // `Create new chat` goes back to the first screen. Nothing was written
        // yet, so nothing is on disk either.
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

    // The side bar lists every place at once, opens a chat of any of them, and
    // puts the chat that was opened at the head of its group. The place of the
    // chat that is open becomes the place the program works in.
    #[test]
    fn the_side_bar_groups_and_orders_the_chats() {
        let _guard = config::test_turn();
        setup();
        write_chat("aaaabbbb01", 100);
        write_chat("aaaabbbb02", 200);

        let engine = Engine::start(Config::default());
        // The first thing the reader sees is the first screen, even when the
        // place has chats. They wait in the side bar.
        let snap = engine.snapshot();
        assert!(snap.home);
        assert!(!snap.generating);
        let lists = engine.lists();
        assert_eq!(lists.loose.len(), 2);
        assert_eq!(lists.loose[0].meta.id, "aaaabbbb02");

        // The chat that stands at the head of the group is not the chat that is
        // open: opening a chat is what the reader does.
        engine.open_chat(String::from("aaaabbbb01"));
        let opened = engine.snapshot();
        assert!(!opened.home);
        assert_eq!(opened.chat_id, "aaaabbbb01");
        // The chat that was opened stands first in its group now, and the rest
        // of the group keeps the order it had.
        let lists = engine.lists();
        assert_eq!(lists.loose[0].meta.id, "aaaabbbb01");
        assert_eq!(lists.loose[1].meta.id, "aaaabbbb02");

        // A new chat is the first screen again, with no chat open.
        engine.new_chat();
        assert!(engine.snapshot().home);

        // A new repository is empty, and the window stands on the first screen
        // while it is empty.
        engine.create_repository(String::from("Test Repo"));
        let in_repo = engine.snapshot();
        assert!(in_repo.in_repository);
        assert!(in_repo.home);

        // A chat of that repository is opened from the side bar, and the place
        // that is open follows it, for the room of a command is the repository
        // of the chat.
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
            .expect("the repository stands in the lists");
        assert_eq!(in_test.1.len(), 1);
        assert_eq!(in_test.1[0].meta.id, "aaaabbbb03");
        // The loose chats are a group of their own, and the chat of the
        // repository is not among them.
        assert!(!lists.loose.iter().any(|c| c.meta.id == "aaaabbbb03"));
    }

    // The menu of a chat renames it, pins it, moves it to another place, puts it
    // away and takes it away.
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

        // The pin. The mark stands with the chat, so it is still there when the
        // chat is read again.
        engine.set_chat_pinned(String::from("aaaacccc01"), true);
        assert!(engine
            .lists()
            .loose
            .iter()
            .any(|c| c.meta.id == "aaaacccc01" && c.meta.pinned));
        // The chat is read from the disk, so the mark came off the file and not
        // only out of the memory.
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

        // A name the reader never gave is not a name of a chat: nothing is
        // changed and nothing is said of a fault either.
        engine.rename_chat(String::from("nosuchchat"), String::from("Nothing"));

        // Put away, and brought back. A chat that is put away stands in no group
        // of the side bar, and stands in the list of the archived chats.
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

        // Move it into a repository, and out of it again. The chat stands in one
        // place at a time, so the file of the place it came from is gone.
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
        assert!(
            !config::paths().chats.join("aaaacccc01.json").exists(),
            "the file of the place it came from is gone"
        );
        engine.move_chat(String::from("aaaacccc01"), String::new());
        assert!(in_loose("aaaacccc01"));

        // A place that is not there is not a place to move a chat to.
        engine.move_chat(String::from("aaaacccc01"), String::from("ffffffffffff"));
        assert!(in_loose("aaaacccc01"));

        // Take the chat away. It is gone from the list and from the disk.
        engine.remove_chat(String::from("aaaacccc01"));
        assert!(!in_loose("aaaacccc01"));
        assert!(engine.lists().archived.is_empty());
        assert_eq!(
            std::fs::read_dir(config::paths().chats)
                .map(|entries| entries.count())
                .unwrap_or(0),
            0
        );
    }

    // The chat that stands open is put away, moved or removed by the side bar
    // too. The window goes to the first screen, for the talk it held is out of
    // the way or gone.
    #[test]
    fn the_open_chat_follows_the_work_of_the_side_bar() {
        let _guard = config::test_turn();
        setup();
        write_chat("aaaadddd01", 100);
        let engine = Engine::start(Config::default());
        engine.open_chat(String::from("aaaadddd01"));
        assert!(!engine.snapshot().home);

        // Put away: the window has nothing to show, so it stands on the first
        // screen.
        engine.set_chat_archived(String::from("aaaadddd01"), true);
        assert!(engine.snapshot().home);
        engine.set_chat_archived(String::from("aaaadddd01"), false);

        // Moved into a repository: the place that is open goes with the chat,
        // and the chat stays open.
        let repo_id = {
            engine.create_repository(String::from("Carried Off"));
            repo_named(&engine, "Carried Off")
        };
        // `Create repository` moves the place that is open, which closes the
        // chat that stood in another place. Open it again, then move it.
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

    // A repository is named again, and the folder it stands in follows the name,
    // for the folder of a repository is named after it. The id does not move, so
    // the talks of the repository keep their place and the chats keep pointing
    // at it. A repository is pinned for the group above the rest.
    #[test]
    fn a_repository_is_named_again_and_pinned() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        engine.create_repository(String::from("First Name"));
        let id = repo_named(&engine, "First Name");
        write_chat_in(&id, "aaaaeeee01", 100);
        let older = config::repositories_dir(&Config::default()).join(format!("first-name_{}", id));
        assert!(older.is_dir(), "the folder of a repository is its name");

        engine.rename_repository(id.clone(), String::from("Second Name"));
        let newer = config::repositories_dir(&Config::default()).join(format!("second-name_{}", id));
        assert!(newer.is_dir(), "the folder went with the name");
        assert!(!older.exists(), "and the older folder is gone");
        let snap = engine.snapshot();
        let shown = snap
            .repositories
            .iter()
            .find(|repo| repo.id == id)
            .expect("the repository is in the bar");
        assert_eq!(shown.name, "Second Name");
        // The talks of the repository did not move, for they stand under the
        // data directory and are keyed on the id.
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

        // The pin. The mark stands in the record of the repository.
        engine.set_repository_pinned(id.clone(), true);
        assert!(engine
            .lists()
            .repositories
            .iter()
            .any(|(repo, _)| repo.id == id && repo.pinned));
        // Taken out of the group above the rest again. The mark stands in the
        // record of this repository alone.
        engine.set_repository_pinned(id.clone(), false);
        assert!(!repo_row(&engine, &id).0.pinned);
    }

    // The name a repository was given, out of the lists the side bar reads. A
    // name that is not there stops the test.
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
        store.save_chat(&chat);
    }

    // Open a chat the way the first message of a reader does. `Create new chat`
    // puts the first screen up, and the words written there start the chat. No
    // model is picked in these tests, so no message goes out; the chat is
    // started in the same way as it is when one does.
    fn open_a_chat(engine: &Engine) {
        engine.new_chat();
        let mut state = engine.state.lock().unwrap();
        state.start_chat();
    }

    // One repository row of the side bar, with the chats that stand in it.
    fn repo_row(engine: &Engine, id: &str) -> (Repo, Vec<Chat>) {
        engine
            .lists()
            .repositories
            .into_iter()
            .find(|(repo, _)| repo.id == id)
            .unwrap_or_else(|| panic!("no repository row for {}", id))
    }

    // The badge offers the modes of the model and the reader chooses one. A
    // mode the model does not offer is refused, and a model with no thinking
    // mode does not change.
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

    // A model with levels and a model with a switch do not offer the same
    // words. Switching from one to the other moves the thinking to a mode the
    // new model offers, so the badge never stands on a word the new template
    // cannot read, and the flags of the server agree with the badge.
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
        // Model A offers the levels, and the reader stands on medium.
        assert!(engine
            .snapshot()
            .thinking_modes
            .contains(&String::from("medium")));
        engine.set_thinking("medium");
        assert_eq!(engine.snapshot().thinking_mode, "medium");
        // Model B knows no levels. The badge moves to off, the chat moves with
        // it, and the server is asked for what the badge shows.
        engine.select_model(String::from("model-b"));
        let snap = engine.snapshot();
        assert_eq!(snap.thinking_mode, "off");
        assert!(!snap.thinking_modes.contains(&String::from("medium")));
        let state = engine.state.lock().unwrap();
        let chat = state.chat().expect("the chat stands open");
        assert_eq!(chat.meta.thinking_mode, "off");
        assert_eq!(state.server_wants().thinking_mode, "off");
        drop(state);
        // Back to the levels: off is a mode both models offer, so it stands,
        // and nothing moves on its own.
        engine.select_model(String::from("model-a"));
        assert_eq!(engine.snapshot().thinking_mode, "off");
        engine.set_thinking("medium");
        assert_eq!(engine.snapshot().thinking_mode, "medium");
    }

    // The choice to keep the thinking of the turns before the last answer
    // stands with the chat, and only a model whose template offers it may
    // change it.
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
            .contains("no setting for keeping the thinking"));
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
            // The line of the day and the clock time that goes before the first
            // message of the chat is not one of the messages this test counts.
            let list = undated(&state.build_history());
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
        open_a_chat(&engine);
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

    // A model that came to its own end without a word is asked again, for the
    // work was never finished; a model that used the room up is not, and neither
    // is a silence that has already been asked over as often as it may be.
    #[test]
    fn a_silence_is_asked_over() {
        // The silence of a model that simply stopped, and has tries left.
        assert!(worth_a_silence("stop", 0, 2));
        assert!(worth_a_silence("stop", 1, 2));
        // The last try was spent, so the quiet line is written.
        assert!(!worth_a_silence("stop", 2, 2));
        // No room for tries at all leaves the silence alone.
        assert!(!worth_a_silence("stop", 0, 0));
        // A silence because the room for the context was used up is not mended by
        // asking again, whatever the number of tries says.
        assert!(!worth_a_silence("length", 0, 2));
    }

    // The thinking of a turn with no words of its own stands with the command
    // it led to, so it is not lost when the line about the answer is left out.
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

    // The question the model put stands a moment after the call was made, for the
    // run of the tool is on a thread of its own. The test looks for it as a window
    // does, and gives up after two seconds of nothing.
    fn asked_of(engine: &Engine) -> Asking {
        for _ in 0..100 {
            if let Some(one) = engine.snapshot().ask {
                return one;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        panic!("no question ever stood above the chat box");
    }

    // A question the model cannot go on without stands above the chat box until
    // the reader answers it, and what they said goes back to the model as the
    // result of the call. The ask goes away with the answer, and answering the
    // same question twice does nothing the second time.
    #[test]
    fn a_question_the_reader_answers_goes_back_to_the_model() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        let (workdir, cancel) = {
            let mut state = engine.state.lock().unwrap();
            state.start_chat();
            let room = state.workdir_for(state.chat().expect("a blank chat stands open"));
            std::mem::drop(state);
            (
                room,
                std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
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
                run_program_tool(&call, "", 0, &held, &std::path::Path::new("."), &waiter)
            })
        };
        // The ask stands while the run waits for a word. A weighty question is
        // waited for as long as it takes, so it carries no time left.
        let asked = asked_of(&engine);
        assert_eq!(asked.question, "Which token should I use?");
        assert_eq!(asked.choices.len(), 2);
        assert!(asked.critical, "the model said it cannot go on");
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
        assert_eq!(run.tool, "The reader answered: Here it is");
        assert!(snap.ask.is_none(), "the ask went away with the answer");
        let _ = std::fs::remove_dir_all(&workdir);
    }

    // A question of the ordinary kind is waited for a short while, and then given
    // up: the model is told that the reader did not answer, and the run is not
    // held up any longer than the setting says.
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
        let held = engine.state.clone();
        let waiter = cancel.clone();
        let worker = {
            let call = call.clone();
            std::thread::spawn(move || {
                run_program_tool(&call, "", 0, &held, &std::path::Path::new("."), &waiter)
            })
        };
        let asked = asked_of(&engine);
        assert!(!asked.critical);
        assert!(asked.choices.is_empty(), "nothing was offered to choose from");
        assert!(asked.left().is_some(), "an ordinary question is timed");
        worker.join().expect("the wait came to an end");
        let snap = engine.snapshot();
        let run = snap
            .messages
            .iter()
            .find(|one| one.tool_name == askuser::NAME)
            .expect("the chat holds the run");
        assert!(run.tool.contains("did not answer within 1 second"), "{}", run.tool);
        assert!(!run.failed, "a question given up is no fault of the run");
        assert!(snap.ask.is_none(), "the ask went away with the wait");
    }

    // A run of the download tool leaves its offer on the message of the run, and
    // the reader reaches the file by the name the block carries. A name that steps
    // out of the room of the chat is answered and never offered, which is the rule
    // of the sandbox over a download as it is over a command.
    #[test]
    fn the_offer_of_a_file_hangs_on_the_run_and_reaches_the_reader() {
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
        run_program_tool(
            &call,
            "",
            0,
            &engine.state,
            &workdir,
            &std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        let snap = engine.snapshot();
        let run = snap
            .messages
            .iter()
            .find(|one| one.tool_name == download::NAME)
            .expect("the chat holds the run");
        assert_eq!(run.role, "Tool");
        assert!(!run.failed, "{}", run.tool);
        assert_eq!(run.downloads.len(), 2, "the two files of the room are offered");
        assert_eq!(run.downloads[0].name, "page");
        assert_eq!(run.downloads[0].extension, "html");
        assert_eq!(run.downloads[0].kind, "page");
        assert_eq!(run.downloads[0].path, "page.html");
        assert_eq!(run.downloads[1].path, "notes/table.csv");
        assert_eq!(run.downloads[1].kind, "table");
        assert!(run.tool.contains("2 files"), "{}", run.tool);
        assert!(run.tool.contains("Not offered"), "{}", run.tool);
        // The state of a page carries the offer with the run, so the blocks of an
        // offer are drawn again when the chat is opened another day.
        let value = crate::web::state(&snap, None);
        let rows = value["messages"].as_array().expect("the state carries the talk");
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
        // The button of a block comes down these two roads. The bytes of a file of
        // the chat come back under the name of the file and the kind of its bytes;
        // a name that leaves the room of the chat, or that names a file which is
        // not there, gives nothing at all.
        let (name, kind, bytes) = engine
            .gift("notes/table.csv")
            .expect("the file stands in the room");
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
            .expect("two of the files stand in the room");
        assert_eq!(called, "page.zip");
        assert_eq!(&kept[0..4], b"PK\x03\x04");
        assert!(engine.gift_zip(&[String::from("nowhere.txt")]).is_none());
        let _ = std::fs::remove_dir_all(&workdir);
    }

    // The picture a run opened for the model hangs on the message of the run. The
    // bytes of a picture are never written into the words of a block, so this is
    // the one way the reader can look at what the model was shown: the state
    // carries the block of the picture with the run, and the bytes stand in the
    // folder of the attachments of the chat until the block asks for them.
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
        assert_eq!(run.attachments.len(), 1, "the run hangs one picture on itself");
        assert!(run.attachments[0].image, "a picture is known as a picture");
        assert_eq!(run.attachments[0].name, "cat.png");

        // The state of a page carries the block of the picture with the run: the
        // name, the kind, and the id the bytes are asked for by. No byte of the
        // picture rides here, for a chat of many pictures would be a wall.
        let value = crate::web::state(&snap, None);
        let rows = value["messages"].as_array().expect("the state carries the talk");
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
            "the state of a page carries no bytes of a picture"
        );

        // The bytes are reached by the id, which is how the block of the run
        // draws the picture the model was shown.
        let record = engine
            .attachment(&run.attachments[0].id)
            .expect("the record of the picture stands with the chat");
        assert_eq!(record.data, "data:image/png;base64,AAA=");
        assert_eq!(record.mime, "image/png");
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
        state.start_chat();
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
        // The line of the day and the clock time is not one of the messages of
        // the talk this test counts.
        let items = undated(&state.build_history());
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
            .find(|t| t.starts_with("The text of attachment 2"))
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

    // The badge of the web search works as the badge of the tools does: with no
    // chat open the choice waits for the next one, and the two switches stand
    // apart, so opening one does not open the other.
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

    // The badge of the tools is the door to every tool that can reach the
    // machine: with it off the model is offered no command and no tool that works
    // on a file. The web search and the mark of a chapter stand on switches of
    // their own, and either of them alone is enough to carry tools with the call.
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
        assert!(offered(false, false, false, tools::Access::default(), &[], &store::ToolSwitches::default()).is_none());
        // The marks alone, which is what a chat with both badges off and the marks
        // on is offered.
        let marks = offered(false, false, true, tools::Access::default(), &[], &store::ToolSwitches::default()).unwrap();
        assert_eq!(names(&marks), vec![tools::CHAPTER_TOOL_NAME.to_string()]);
        // The badge on: every tool of the program stands there, and the mark is
        // not among them.
        let open = offered(true, false, false, tools::Access::default(), &[], &store::ToolSwitches::default()).unwrap();
        let words = names(&open);
        assert!(words.contains(&tools::TOOL_NAME.to_string()));
        assert!(words.contains(&editfile::EDIT_NAME.to_string()));
        assert!(words.contains(&editfile::CREATE_NAME.to_string()));
        assert!(words.contains(&viewimage::NAME.to_string()));
        assert!(!words.contains(&tools::CHAPTER_TOOL_NAME.to_string()));
        assert!(!words.contains(&search::NAME.to_string()));
        // The badge of the web search alone: the search is offered, and none of
        // the tools that can reach the machine is offered with it.
        let looked = offered(false, true, false, tools::Access::default(), &[], &store::ToolSwitches::default()).unwrap();
        assert_eq!(names(&looked), vec![search::NAME.to_string()]);
        // Both badges stand open, so both doors are open at once.
        let both = offered(true, true, false, tools::Access::default(), &[], &store::ToolSwitches::default()).unwrap();
        let words = names(&both);
        assert!(words.contains(&tools::TOOL_NAME.to_string()));
        assert!(words.contains(&search::NAME.to_string()));
        // What the chat lets a command reach stands in the words of the tool, so
        // the model knows it before it asks for a command.
        let open = offered(
            true,
            false,
            false,
            tools::Access { network: true, pip: true, fetch: true },
            &[],
            &store::ToolSwitches::default(),
        )
        .unwrap();
        let said = String::from(open[0]["function"]["description"].as_str().unwrap_or(""));
        assert!(said.contains("The command may reach the network."));
        // Every tool of the list carries the rule of when to call it. A model
        // that is not told it may answer in words spends the first turn of a chat
        // arguing with the list it was given, and writes the argument out as
        // thinking before an answer that goes on as if none of it had been said.
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

    // The tools of the servers of the reader's own stand in the list of a step,
    // and a server alone is enough to let the model be offered anything at all.
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
        let list = offered(false, false, false, tools::Access::default(), &made,
            &store::ToolSwitches::default())
            .expect("a server of the reader's own is a tool");
        let names: Vec<&str> = list
            .as_array()
            .unwrap()
            .iter()
            .map(|one| one["function"]["name"].as_str().unwrap_or(""))
            .collect();
        assert_eq!(names, vec!["mcp__bookshop__search_books"]);
        // A server is reached out of the program and never inside the jail, so
        // its words say so, and the rule of when to call it stands with it.
        let said = list[0]["function"]["description"].as_str().unwrap_or("");
        assert!(said.contains("Search the books of the shop."));
        assert!(said.contains("Bookshop"));
        assert!(said.contains(tools::CALL_RULE));
    }

    // The three settings of the sandbox stand off for a new chat, and every one
    // of them is kept for the next chat as well as for the one in hand. An
    // install or a fetch opens the network, for neither does anything without it,
    // and closing one leaves the network to the other.
    #[test]
    fn the_settings_of_the_sandbox_stand_off() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        // A new set of settings, and a chat written before the three existed, both
        // read with all three off.
        assert_eq!(tools::Access::default(), tools::Access { network: false, pip: false, fetch: false });
        let older = store::ChatMeta::default();
        assert!(!older.sandbox_network && !older.sandbox_pip && !older.sandbox_fetch);
        assert_eq!(access_of(&older), tools::Access::default());

        // Nothing is open at the start, whatever the sandbox itself stands on.
        let first = engine.snapshot();
        assert_eq!(first.sandbox_access, tools::Access::default());
        // The three stand apart: an install does not open the switch of the
        // network, and it does not open the fetch either. What a command may
        // really reach is the three of them together.
        engine.set_sandbox_setting(SandboxSetting::Pip, true);
        let said = engine.snapshot();
        assert!(said.sandbox_access.pip);
        assert!(!said.sandbox_access.network);
        assert!(!said.sandbox_access.fetch);
        assert!(said.sandbox_access.net(), "an install needs the network");
        // Closing the install leaves nothing open at all.
        engine.set_sandbox_setting(SandboxSetting::Pip, false);
        assert_eq!(engine.snapshot().sandbox_access, tools::Access::default());
        // A fetch stands on its own, and an install beside it is told apart from
        // it.
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

    // The head of a block, and the block of a call being written, take the one
    // thing the call names: the command of a run, or the file a file tool works
    // on. A reason of the call rides along for the terminal tool alone.
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
        // The head names the file the call works on, and the body holds the
        // words of the change the call is making.
        assert_eq!(made[0].target, "notes/todo.txt");
        assert_eq!(made[0].command, "one\n> two");
        assert!(made[0].purpose.is_empty(), "{}", made[0].purpose);
        // A name of a file that is still coming in is shown as far as it has
        // come, as a command is, and the words of a new file stand in the body
        // as they come.
        let half = ToolCall {
            id: String::from("call-half"),
            name: String::from(editfile::CREATE_NAME),
            arguments: String::from("{\"path\":\"notes/plan.txt\",\"text\":\"hello\"}"),
        };
        let half_made = watched(std::slice::from_ref(&half));
        assert_eq!(half_made[0].target, "notes/plan.txt");
        assert_eq!(half_made[0].command, "hello");
        // A name the program does not offer is shown with the file it names, so
        // the reader is not left looking at an empty block.
        let strange = ToolCall {
            id: String::from("call-strange"),
            name: String::from("rewrite"),
            arguments: String::from("{\"path\":\"notes/todo.txt\"}"),
        };
        assert_eq!(watched(std::slice::from_ref(&strange))[0].command, "notes/todo.txt");
        // A model that writes the halves of a call in the order it likes, and not
        // in the order the shape of the call asks for them, is shown as far as
        // each of them has come. The words of the change stand in the body from
        // the first of them, so the block is not blank while the call is written.
        let backwards = ToolCall {
            id: String::from("call-backwards"),
            name: String::from(editfile::EDIT_NAME),
            arguments: String::from("{\"edits\":[{\"new_string\":\"un"),
        };
        let read = watched(std::slice::from_ref(&backwards));
        assert_eq!(read[0].command, "> un");
        assert_eq!(read[0].target, String::new());
        let half_read = ToolCall {
            id: String::from("call-half-read"),
            name: String::from(editfile::EDIT_NAME),
            arguments: String::from("{\"edits\":[{\"new_string\":\"uno\",\"old_string\":\"on"),
        };
        assert_eq!(watched(std::slice::from_ref(&half_read))[0].command, "on\n> uno");
        // The name of the file is the head of the block whichever half of the
        // call it stands in, so a model that writes the changes first names the
        // file as soon as the name begins.
        let named_last = ToolCall {
            id: String::from("call-named-last"),
            name: String::from(editfile::EDIT_NAME),
            arguments: String::from(
                "{\"edits\":[{\"new_string\":\"uno\",\"old_string\":\"one\"}],\"path\":\"notes/to",
            ),
        };
        let read = watched(std::slice::from_ref(&named_last));
        assert_eq!(read[0].target, "notes/to");
        assert_eq!(read[0].command, "one\n> uno");
    }

    // The calls being written stand in the snapshot, and go away with the step
    // that wrote them, so a block is never left standing over a chat.
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

    // A model that thinks, writes a call, and then thinks again has finished the
    // words of that call long before the step of the answer comes to an end. The
    // block of it must stop saying it is being written, and must keep saying it
    // is written while nothing new comes of those words. Words that grow again
    // are being written again, and say so.
    #[test]
    fn a_call_the_model_moved_on_from_says_it_is_written() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        open_a_chat(&engine);
        let mut state = engine.state.lock().unwrap();
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
        // The words of the call are coming in.
        write_calls(&mut state, std::slice::from_ref(&half));
        assert_eq!(state.live_tools.len(), 1);
        assert!(!state.live_tools[0].done);
        // The model thinks again, so the call is written.
        assert!(end_writing_calls(&mut state));
        assert!(state.live_tools[0].done);
        // A piece that brings nothing new does not start the writing again.
        write_calls(&mut state, std::slice::from_ref(&half));
        assert!(state.live_tools[0].done);
        // Words that grew are being written again.
        write_calls(&mut state, std::slice::from_ref(&whole));
        assert!(!state.live_tools[0].done);
        // Nothing stood written, so ending the writing is a change the window
        // has to hear about.
        assert!(end_writing_calls(&mut state));
        assert!(state.live_tools[0].done);
        // The same word again is nothing to tell.
        assert!(!end_writing_calls(&mut state));
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
        open_a_chat(&engine);
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
        open_a_chat(&engine);
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

    // The meter of the context moves at most twice a second. The server speaks
    // of the talk on every piece of the answer, which is a great deal oftener
    // than the reader can read a number. A number that is turned away is not
    // lost: the next piece after the half second brings the talk up to date, and
    // a number that goes back is turned away at any moment.
    #[test]
    fn the_meter_of_the_context_moves_every_half_second() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        open_a_chat(&engine);
        let begun = Instant::now();
        let mut state = engine.state.lock().unwrap();
        state.ctx_used = 0;
        state.ctx_at = None;
        // The first number of the answer is put in at once.
        assert!(climb_context_paced(&mut state, 100, begun));
        assert_eq!(state.ctx_used, 100);
        // Within the half second the meter stands where it stood.
        assert!(!climb_context_paced(&mut state, 120, begun + Duration::from_millis(120)));
        assert!(!climb_context_paced(&mut state, 138, begun + Duration::from_millis(499)));
        assert_eq!(state.ctx_used, 100);
        // Past the half second the newest number is stood on, and the pieces
        // that were turned away are not asked for again.
        assert!(climb_context_paced(&mut state, 140, begun + Duration::from_millis(501)));
        assert_eq!(state.ctx_used, 140);
        // A number that goes back never moves the meter, pace or no pace.
        assert!(!climb_context_paced(&mut state, 90, begun + Duration::from_secs(5)));
        assert_eq!(state.ctx_used, 140);
    }

    // ------------------------------------------------------------ compaction
    // One message of a talk, put in the chat that stands open.
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

    // A handoff summary is written by a model, and a model wraps what it writes
    // in words of its own. Those words are the envelope of the summary rather
    // than the summary, and they are sent again on every turn after the
    // compaction, so they are taken off.
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
        // Words that are the summary keep their marks, for the next model reads
        // them as the summary: a fence of code inside a summary is part of what
        // was done, and only a fence about the whole of it is the wrapper.
        let plain = "Done: fixed `parser.rs`.\n\nNext: the bar.";
        assert_eq!(clean_summary(plain), "Done: fixed `parser.rs`.\n\nNext: the bar.");
        // Blank lines one behind the other say nothing, and the next model pays
        // for every one of them.
        assert_eq!(clean_summary("one\n\n\n\n\ntwo"), "one\n\ntwo");
        // A line that is the summary is not a lead-in, whatever it begins with.
        let told = "Here is what the session did, in the words of the model.";
        assert_eq!(clean_summary(told), told);
    }

    // The words that go round a handoff summary tell the next model what it is
    // reading, so that it goes on with the work instead of greeting the reader
    // or beginning it again.
    #[test]
    fn a_handoff_summary_is_named_for_the_model_that_reads_it() {
        let block = handoff_block("Done: the parser. Next: the bar.");
        assert!(block.contains("The session was compacted"));
        assert!(block.contains("handoff summary"));
        assert!(block.contains("Do not greet the user"));
        assert!(block.ends_with("Done: the parser. Next: the bar."));
    }

    // The summarisations of a chat stand where they were made, are numbered in
    // the order they were made, and say what became of each of them.
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
        // The second summarisation, asked for and never written.
        let second = state.begin_summary();
        let marks = state.handoffs();
        assert_eq!(marks.len(), 2);
        assert_eq!(marks[0].number, 1);
        assert_eq!(marks[0].state, "done");
        assert_eq!(marks[0].before, 6964);
        assert_eq!(marks[0].after, 412);
        assert!(marks[0].snippet.starts_with("Done: the parser"));
        assert!(marks[0].why.is_empty());
        // Nothing is being written, so an empty summarisation is one that is
        // owed: the chat stands at that place until the words come.
        assert_eq!(marks[1].number, 2);
        assert_eq!(marks[1].state, "owed");
        assert!(marks[1].snippet.is_empty());
        assert!(state.handoff_owed());
        // While the program is at a summary, the empty mark is not yet a fault.
        state.generating = true;
        assert_eq!(state.handoffs()[1].state, "writing");
        state.generating = false;
        // The mark that was never written is the newest thing of the chat, so
        // there is nothing new to write a summary of.
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

    // A summarisation that was never written is started again in the place it
    // stands. The reader was told to write that one again, so a new heading with
    // a new number would say the chat was summarised twice when it was not. The
    // mark keeps its number, and the chat is not held by an empty mark of its own
    // once the words come.
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
        // The second summarisation was asked for and never written.
        let second = state.begin_summary();
        state.finish_summary(&second, "", 7000, 0, "the server went away");
        assert!(state.handoff_owed());
        assert_eq!(state.handoffs().len(), 2);
        // Started again: the same mark, the same number, and no third heading.
        let again = state.begin_summary();
        assert_eq!(again, second);
        let marks = state.handoffs();
        assert_eq!(marks.len(), 2);
        assert_eq!(marks[1].number, 2);
        state.generating = true;
        assert_eq!(state.handoffs()[1].state, "writing");
        state.generating = false;
        state.finish_summary(&again, "Done: the bar. Next: nothing.", 7000, 96, "");
        assert_eq!(state.handoffs().len(), 2);
        assert_eq!(state.handoffs()[1].number, 2);
        assert_eq!(state.handoffs()[1].state, "done");
        // A mark of long ago that was never written must not be written into for
        // a new summarisation. Only the newest mark is started again, and a fresh
        // summarisation is a fresh mark. The old mark is put before the mark that
        // came off, so it is not the newest thing in the talk.
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
        // The newest mark holds words, so the chat is free again.
        assert!(!state.handoff_owed());
        let third = state.begin_summary();
        assert_ne!(third, "s-old");
        // The two marks of the talk, the mark of long ago, and this new one.
        let marks = state.handoffs();
        assert_eq!(marks.len(), 4);
        assert_eq!(marks[1].id, "s-old");
        assert_eq!(marks[1].state, "owed");
        assert_eq!(marks[3].id, third);
        assert_eq!(marks[3].number, 4);
        // The new mark holds the chat until its words come, and no longer after.
        assert!(state.handoff_owed());
        state.finish_summary(&third, "Done: nothing left to do.", 7000, 64, "");
        assert!(!state.handoff_owed());
    }

    // A chat that was summarised sends the first message of the reader, the
    // handoff summary, and the words that came after the summary. The words the
    // summary stands for are left out, which is the room the compaction made.
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
        // The line of the day and the clock time that stands before the first
        // message of the chat is not one of the messages of the talk here.
        let before = undated(&state.build_history());
        assert_eq!(before.len(), 3);
        let mark = state.begin_summary();
        state.finish_summary(&mark, "Done: the parser. Next: the bar.", 6964, 412, "");
        // A chat that ends on a summarisation asks the model from behind it: the
        // handoff block is the last word of the talk, which is what the model is
        // put back to work on when the summary is written, and the first message
        // of the reader is still the first thing in front of it.
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
        // The first message of the reader keeps the place it had, and the words
        // of it are the same words, byte for byte. The server worked them
        // through once and keeps that work, which is why the summary rides after
        // that message instead of in the place of it.
        assert_eq!(items[0], before[0]);
        assert_eq!(items[0]["role"], json!("user"));
        assert_eq!(items[0]["content"], json!("the first prompt"));
        // The summary stands second, wrapped in the words that name it.
        assert_eq!(items[1]["role"], json!("user"));
        let text = items[1]["content"].as_str().expect("words");
        assert!(text.contains("The session was compacted"));
        assert!(text.contains("Done: the parser."));
        // Then the words that came after the summary, and nothing else: the old
        // question and the long answer it was answered with are gone from the
        // prompt, for the summary stands for them.
        assert_eq!(items[2]["content"], json!("and the next thing"));
        assert_eq!(items[3]["role"], json!("assistant"));
        assert_eq!(items.len(), 4);
        assert!(!json!(items).to_string().contains("the old answer"));
        assert!(!json!(items).to_string().contains("an older question"));
        // The line of the day and the clock time still stands at the head of the
        // talk after the compaction, before the first message of the reader, and
        // the first message is still sent word for word.
        let said = state.build_history();
        let lines = date_lines(&said);
        assert_eq!(lines.len(), 1, "one line of the day: {:?}", lines);
        let head = said
            .as_array()
            .expect("a list of the talk")
            .first()
            .expect("the talk is not empty");
        assert!(head["content"]
            .as_str()
            .expect("a line of the day first")
            .starts_with(DATE_HEAD));
        assert_eq!(undated(&said)[0]["content"], json!("the first prompt"));
        // The reader still sees the whole of the talk: a summarisation is a mark
        // in it, and no word is thrown away. The lock of the state is let go
        // first, for the snapshot takes it and a lock is not taken twice.
        drop(state);
        let snap = engine.snapshot();
        assert_eq!(snap.messages.len(), 6);
        // No summary is being written here, so the window is free to draw a live
        // answer. While a summary is written it draws none.
        assert!(!snap.summarising);
        assert!(snap
            .messages
            .iter()
            .any(|m| m.role == "Summary" && m.context_before == 6964));
    }

    // The room ran full under a summary, so it stops in the middle of itself and
    // is asked again for a short one. A stop of the reader's own and a fault of
    // the stream are not: the first has nothing to shorten, and the other is told
    // to the reader, who starts the summary again.
    #[test]
    fn a_summary_that_ran_to_the_end_of_the_room_is_asked_again_for_a_short_one() {
        let shot = |finish: &str, stopped: bool, error: Option<&str>| SummaryShot {
            text: String::from("a summary that stops in the middle of a senten"),
            finish: String::from(finish),
            calls: Vec::new(),
            stopped,
            error: error.map(String::from),
        };
        // The room ran full: ask again, of the short prompt.
        assert!(summary_too_long(&shot("length", false, None)));
        // The model stopped of its own accord, so the summary is whole.
        assert!(!summary_too_long(&shot("stop", false, None)));
        assert!(!summary_too_long(&shot("", false, None)));
        // The reader stopped the ask, and the stream broke: neither is asked
        // again for a shorter one.
        assert!(!summary_too_long(&shot("length", true, None)));
        assert!(!summary_too_long(&shot("length", false, Some("the server went away"))));
    }

    // A model that answers the ask for a summary with a call of a tool, cut short
    // by the room, has written nothing the next model can use. Those words are
    // thrown away, so that the ask is made again rather than a call of a tool
    // standing in the place of a summary.
    #[test]
    fn a_call_of_a_tool_is_no_summary() {
        let cut = "Done: the page was written.\n\n<|im_start|toolcall\n{\"name\": \"web_search\", \"arguments\": {\"query\": \"Helluva Boss plot summary I.M.P assassin business Stolas romance Goet";
        assert!(clean_summary(cut).is_empty());
        // Words that shut their mark again are a saying of the model after all, and
        // are kept as they were written.
        let told = "Done: the page was written. Next: nothing.";
        assert_eq!(clean_summary(told), told);
    }

    // The ask for a handoff summary sets no ceiling over the words the model may
    // write. A summary is the making of the room and not the spending of it, so a
    // ceiling of 4096 in the middle of a long summary cut it off in the middle of
    // a sentence; the model now writes as long as it likes, and an ask that runs
    // to the end of the room is made again for a shorter summary. The number sent
    // is -1, which also lifts a `--n-predict` out of the settings of the model.
    #[test]
    fn the_ask_for_a_summary_sets_no_ceiling_over_the_words() {
        let talk = json!([
            { "role": "user", "content": "the first prompt" },
            { "role": "assistant", "content": "an answer of some length" },
        ]);
        let ask = summary_ask(&talk, "Summarise it.", None);
        assert_eq!(ask["max_tokens"], json!(NO_SUMMARY_CEILING));
        assert_eq!(ask["max_tokens"].as_i64(), Some(-1));
        // The same for a talk that has eaten the whole window: the ask is not
        // clipped to a room of its own, and it is not grown either.
        let huge = "x".repeat(4 * 16000);
        let ask = summary_ask(&json!([{ "role": "user", "content": huge }]), "Summarise it.", None);
        assert_eq!(ask["max_tokens"], json!(NO_SUMMARY_CEILING));
        // The words of the talk all ride with the ask, and the ask of the
        // summary is the last message of them.
        let items = ask["messages"].as_array().expect("a list of the talk");
        assert_eq!(items.len(), 2);
        assert_eq!(ask["stream"], json!(true));
    }

    // A model that reads a session full of tool calls again is minded to make one,
    // and a call made instead of the summary spends the very room the summary was
    // come to make. The tools cannot simply be left out of the ask -- the list of
    // them is part of the front of the words the server has worked through -- so
    // the ask says in its own words that nothing here is to be called, whatever
    // the reader set the handoff prompt to.
    #[test]
    fn the_ask_for_a_summary_says_not_to_call_any_tool() {
        let talk = json!([{ "role": "user", "content": "the first prompt" }]);
        let ask = summary_ask(&talk, "Write a handoff summary.", None);
        let items = ask["messages"].as_array().expect("a list of the talk");
        let last = items[items.len() - 1]["content"]
            .as_str()
            .expect("the words of the ask");
        // The words of the reader stand as they wrote them, and the guard rides
        // after them.
        assert!(last.starts_with("Write a handoff summary."));
        // The guard is appended to whatever the reader set, so the word against
        // calling stands in every ask, of every prompt.
        assert!(last.contains("Do not call any tools."));
        assert!(last.contains("never a call and never the marker of one"));
        // The ask names a length, for a summary that fills the window leaves the
        // room it was asked to make unmade.
        assert!(last.contains("three hundred words"));
        // The first message of the reader is still the first message, for the
        // server keeps the work it did on those words.
        assert_eq!(items[0]["content"], json!("the first prompt"));
        assert_eq!(ask["stream"], json!(true));
        // The server is no longer asked to leave the calls unread. A call that
        // comes is caught and cancelled, and a server told not to read them at
        // all would hide one.
        assert!(ask.get("parse_tool_calls").is_none());
    }

    // The list of the tools is rendered into the front of the talk, so the ask for
    // a summary must carry the very list an answer of this chat carries. An ask
    // with another list, or with none, is the whole talk worked through again from
    // its first word, which is what this ask used to do.
    #[test]
    fn the_ask_for_a_summary_carries_the_tools_of_the_talk() {
        let talk = json!([{ "role": "user", "content": "the first prompt" }]);
        let tools = json!([{ "type": "function", "function": { "name": "run_terminal" } }]);
        let ask = summary_ask(&talk, "Summarise it.", Some(&tools));
        assert_eq!(ask["tools"], tools);
        assert_eq!(ask["tool_choice"], json!("auto"));
        // A chat with nothing offered offers nothing, as its answers do.
        let ask = summary_ask(&talk, "Summarise it.", None);
        assert!(ask.get("tools").is_none());
        assert!(ask.get("tool_choice").is_none());
    }

    // The bytes of a picture cost a great deal of room, and they used to be taken
    // out of the talk for the ask. That saving was paid for many times over: the
    // picture stands among the words the server has already worked through, and
    // taking it out made a front the server had never seen, so the whole talk went
    // through again. The pictures ride with the ask now, as they ride with every
    // other message of the talk.
    #[test]
    fn the_ask_for_a_summary_keeps_the_pictures_of_the_talk() {
        let talk = json!([
            { "role": "user", "content": "the first prompt" },
            { "role": "user", "content": [ { "type": "text", "text": "look at this" },
                                            { "type": "image_url", "image_url": { "url": "data:image/png;base64,AAA=" } } ] },
        ]);
        let ask = summary_ask(&talk, "Summarise it.", None);
        let items = ask["messages"].as_array().expect("a list of the talk");
        // The message the picture hangs on stands part for part as it stood.
        let parts = items[1]["content"].as_array().expect("the parts of the message");
        assert_eq!(parts, talk[1]["content"].as_array().expect("the same parts"));
        assert_eq!(parts[1]["type"], json!("image_url"));
        // The ask is the talk and one message more, which is all the server has
        // that it has not seen before.
        assert_eq!(items.len(), 3);
    }

    // A talk full of tool runs is the commonest talk of a working chat, and the
    // ask for a summary of it used to carry it made over: the calls and the
    // receipts of them turned into lines of words. Every one of those cuts fell in
    // the middle of the words the server had worked through, and the whole talk
    // went through again for it. The ask carries the talk as it stands, so the
    // server keeps its work and has only the ask to read anew.
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
        let items = ask["messages"].as_array().expect("a list of the talk");
        // Every message of the talk stands in the ask as it stood in the talk,
        // calls, receipts and all.
        let told = talk.as_array().expect("the talk");
        assert_eq!(items.len(), told.len() + 1);
        for (place, message) in told.iter().enumerate() {
            assert_eq!(&items[place], message, "the message at {} was changed", place);
        }
        // The ask is the last message, and it is a message of the reader.
        let last = items[items.len() - 1].clone();
        assert_eq!(last["role"], json!("user"));
        assert!(last["content"].as_str().expect("words").starts_with("Summarise it."));
    }

    // A talk that ends on a call, or on the receipt of one, used to end the ask on
    // a word of the model once the calls were gone, and the server was then asked
    // to go on where the model left off instead of to summarise it. Nothing is
    // taken out of the talk now, and the ask is itself a message of the reader: the
    // ask ends the talk whatever the talk ends on.
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
        let items = ask["messages"].as_array().expect("a list of the talk");
        let last = items[items.len() - 1].clone();
        assert_eq!(last["role"], json!("user"));
        assert!(last["content"].as_str().expect("words").starts_with("Summarise it."));
        // The receipt of the last run is the message before the ask, so the model
        // reads the talk to its end and the ask after it.
        let before = items[items.len() - 2].clone();
        assert_eq!(before["role"], json!("tool"));
    }

    // A model that answers the ask for a summary with a call instead of with the
    // summary is answered as any other run is answered: the call is cancelled, the
    // reason is given, and the ask goes on from where it stood. The pair of entries
    // is shaped as a call and its receipt are shaped, for a server reads a call
    // with no receipt as a debt it still has to pay.
    #[test]
    fn a_call_made_instead_of_a_summary_is_answered_as_a_cancelled_run() {
        let calls = vec![
            llama::ToolCall {
                id: String::from("c1"),
                name: String::from("run_terminal"),
                arguments: String::from("{\"command\":\"ls\"}"),
            },
            // A server that wrote no id for a call must still be answered with
            // one, and the same one on both sides of the pair.
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
        let given = made[0]["tool_calls"].as_array().expect("the calls as they stood");
        assert_eq!(given.len(), 2);
        assert_eq!(given[0]["id"], json!("c1"));
        assert_eq!(given[1]["function"]["name"], json!("search_web"));
        // The receipts name the call they answer and say why it never ran.
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
        // A model that asked without writing any words beside the call is given
        // back as a saying of nothing, as every other call of that shape is.
        let silent = summary_calls_answered(&calls[..1], "   ");
        assert_eq!(silent[0]["content"], Value::Null);
    }

    // The talk of a chat that ends on a call of a tool cannot be sent as it
    // stands: the server has a debt to pay and pays it with a call of its own.
    // The ask that puts the model back to work behind a handoff summary therefore
    // carries a word of its own, and the talk ends on a question again. This is
    // how such a talk is known, and how it stops being one the moment the model
    // writes words of its own after the run.
    #[test]
    fn the_talk_after_a_summarisation_ends_on_a_question_after_a_call() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        let mut state = engine.state.lock().unwrap();
        state.start_chat();
        add_message(&mut state, "You", "the first prompt");
        // A run whose command has run: the call and its receipt stand in the
        // talk, and the server has the receipt to read. The id of the call is
        // what a real answer carries, and a run with none sends nothing at all.
        let mut call = cut_call(tools::TOOL_NAME, "{\"command\":\"ls\"}");
        call.id = String::from("call-1");
        let id = state.begin_tool(&call, "ls", "", 0);
        state.finish_tool(&id, "notes.txt todo.txt", true);
        // The line of the day and the clock time stands at the head of the talk,
        // so the words of the talk are counted without it.
        let items = undated(&state.build_history());
        assert!(
            ends_on_a_call(&json!(items)),
            "a talk that ends on a call was not known: {}",
            json!(items)
        );
        // A word of the model after the run, and the talk may be sent as it
        // stands: nobody is waiting for an answer any more.
        add_message(&mut state, "Assistant", "two files stand here");
        let items = undated(&state.build_history());
        assert!(
            !ends_on_a_call(&json!(items)),
            "a plain turn was taken for a call: {}",
            json!(items)
        );
    }

    // The telling of a call that the room gave out under stands in the place of the
    // turn that was cut short, and asks the model for that call again. The talk that
    // ends on it is therefore a call the server owes an answer to, as a run whose
    // receipt stands last is. Without a word of the reader after it a server set to
    // read calls writes one of its own, the words of it are thrown away with the ask
    // that made it, and the reader is left with no answer at all.
    #[test]
    fn the_telling_of_a_cut_call_asks_the_server_for_an_answer() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        let mut state = engine.state.lock().unwrap();
        state.start_chat();
        add_message(&mut state, "You", "the first prompt");
        let cut = cut_call(editfile::EDIT_NAME, "{\"path\":\"notes.md\"");
        state.push_cancelled(&cut, "", 0, GREATEST_CUT_ARGS);
        let items = undated(&state.build_history());
        // The telling stands last, as the words of the turn it stood in, and the
        // server is owed the call it tells of.
        let last = items.last().expect("a talk with the telling in it");
        assert_eq!(last["role"], json!("assistant"));
        assert!(last["content"]
            .as_str()
            .expect("words")
            .starts_with(CUT_TOLD_FIRST));
        assert!(
            ends_on_a_call(&json!(items)),
            "the telling of a call was not known as a call: {}",
            json!(items)
        );
        assert!(
            cut_asks_for_a_call(&json!(items)),
            "the ask was not known to carry a debt: {}",
            json!(items)
        );
        // A run of a tool, and the talk ends on the receipt of it: that is a debt of
        // another kind, and no word of the cut call stands at the end of it.
        let mut ran = cut_call(tools::TOOL_NAME, "{\"command\":\"ls\"}");
        ran.id = String::from("call-9");
        let id = state.begin_tool(&ran, "ls", "", 0);
        state.finish_tool(&id, "notes.md", true);
        let items = undated(&state.build_history());
        assert!(ends_on_a_call(&json!(items)), "a receipt was not a debt");
        assert!(
            !cut_asks_for_a_call(&json!(items)),
            "a plain receipt was taken for the telling of a cut call: {}",
            json!(items)
        );
        // A word of the model after the telling, and the ask may go out as the talk
        // stands: nobody waits for the call any more.
        add_message(&mut state, "Assistant", "I will write it in pieces");
        let items = undated(&state.build_history());
        assert!(!ends_on_a_call(&json!(items)));
        assert!(!cut_asks_for_a_call(&json!(items)));
        // The word of the reader that pays the debt is not the telling itself, so a
        // talk that ends on it is no debt.
        let asked = go_on_words(&state, CUT_CONTINUE_WORDS);
        let items = undated(&asked);
        assert_eq!(items.last().expect("a talk")["role"], json!("user"));
        assert!(!cut_asks_for_a_call(&json!(items)));
    }

    // The word that pays the debt of a cut call is a word of the program and not of
    // the reader, so it stands in no message of the chat: the reader never wrote it
    // and has nothing to read where it stands. It rides on the ask alone, and comes
    // to no window.
    #[test]
    fn the_word_that_pays_the_debt_of_a_cut_call_never_reaches_the_window() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        let mut state = engine.state.lock().unwrap();
        state.start_chat();
        add_message(&mut state, "You", "the first prompt");
        let cut = cut_call(editfile::EDIT_NAME, "{\"path\":\"notes.md\"");
        state.push_cancelled(&cut, "", 0, GREATEST_CUT_ARGS);
        // The word goes out with the ask, and the talk ends on the reader.
        let asked = undated(&go_on_words(&state, CUT_CONTINUE_WORDS));
        let last = asked.last().expect("a talk with the word in it");
        assert_eq!(last["role"], json!("user"));
        assert_eq!(last["content"].as_str().expect("words"), CUT_CONTINUE_WORDS);
        // Nothing of it stands in the chat, so nothing of it can come to the window.
        let said = json!(state.snap().messages).to_string();
        assert!(
            !said.contains(CUT_CONTINUE_WORDS),
            "the word of the program came to the window"
        );
        // The words of the cut call are told once, and once only: the word of the
        // program names the debt without repeating the telling of it.
        let told = asked
            .iter()
            .filter(|item| {
                item["content"]
                    .as_str()
                    .unwrap_or_default()
                    .starts_with(CUT_TOLD_FIRST)
            })
            .count();
        assert_eq!(told, 1, "the telling of the call was carried twice");
    }

    // The ask of a turn that only goes on carries a word of its own when the talk
    // ends on a call or on the receipt of one, so the talk the server is given ends
    // on a question. The reader never wrote that word, so it is not written into the
    // chat at all: it rides on the ask alone, and the window draws no bubble of it
    // between the summarisation and the answer. Where the talk ends on a word of the
    // reader already -- the handoff block of a summarisation that stands last -- no
    // word is added, for the block is in the talk already, and a second copy of it
    // costs the room the summarisation was made to win.
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
        state.finish_summary(&mark, "The talk was about one thing.", 3000, 300, "");
        // The mark of the summarisation stands last, so the talk ends on its handoff
        // block and is a question already. An ask with no word to carry is the talk
        // as it stands, with nothing added to it.
        let stood = state.build_history();
        assert_eq!(
            go_on_words(&state, ""),
            stood,
            "words were added to a talk that ends on a question already"
        );
        // A run of a tool after the mark. The talk now ends on the receipt of it, so
        // the ask carries the word of the program after it, and ends on a question.
        let mut call = cut_call(tools::TOOL_NAME, "{\"command\":\"ls\"}");
        call.id = String::from("call-2");
        let ran = state.begin_tool(&call, "ls", "", 0);
        state.finish_tool(&ran, "notes.txt todo.txt", true);
        let debt = state.build_history();
        assert!(ends_on_a_call(&debt), "a receipt was not known as a debt: {}", debt);
        let history = go_on_words(&state, CONTINUE_PROMPT);
        let items = undated(&history);
        let last = items.last().expect("a talk the ask was given");
        assert_eq!(last["role"], json!("user"), "the ask did not end on the reader");
        assert_eq!(last["content"].as_str().expect("words"), CONTINUE_PROMPT);
        assert!(!ends_on_a_call(&json!(items)), "the ask still ends on a call: {}", history);
        // The handoff block of the summarisation is carried once, and once only.
        let block = handoff_block("The talk was about one thing.");
        let carried = items
            .iter()
            .filter(|item| item["content"].as_str().unwrap_or_default() == block.as_str())
            .count();
        assert_eq!(carried, 1, "the handoff block was not carried once: {}", history);
        // Nothing of the word stands in the chat, so nothing of it can come to the
        // window.
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

    // A chat that was cut for a handoff summary and never got one does not go on
    // asking the model, for its talk is past the room of the model. The reader
    // starts the summary again from the place it stood at.
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
        // A chat with no summary waiting says so, rather than asking a model for
        // a summary of nothing.
        engine.restart_handoff();
        // The summary of this chat is waiting, and no model stands in the top
        // bar, so the word of the model is what the reader is given.
        assert!(engine.snapshot().status.contains("Choose a model"));
    }

    // ------------------------------------------------- a call cut short
    //
    // A model writing a tool call bigger than the room it has writes until the
    // room is gone. The program stops the writing at the limit it was set, and
    // three things have to come of it: the reader sees the call that never ran,
    // the model is told it was cancelled and what it had written, and neither of
    // those two sayings breaks the next ask.

    // One call, stopped in the middle of its words, as the stream leaves it: the
    // name is there, the arguments stop under a string that was never shut.
    fn cut_call(name: &str, arguments: &str) -> ToolCall {
        ToolCall {
            id: String::new(),
            name: String::from(name),
            arguments: String::from(arguments),
        }
    }

    // ------------------------------------------- the day before the model
    //
    // The Chatting settings can put the day and the clock time before the model:
    // at the start of a chat, and with every message of the reader. A line stands
    // in a message of its own just before the message it dates, so the words of
    // the reader go out as they were written, and the chat on disk and the page
    // know nothing of the line.
    fn dated_chat(state: &mut State, cfg: &Config) {
        state.cfg = cfg.clone();
        state.start_chat();
        add_message(state, "You", "the first prompt");
        add_message(state, "Assistant", "the first answer");
        add_message(state, "You", "the second prompt");
        add_message(state, "Assistant", "the second answer");
        add_message(state, "You", "the third prompt");
    }

    // The talk without the lines of the day and the clock time, so a test about
    // another rule counts the words of the reader and the model as they were.
    fn undated(history: &Value) -> Vec<Value> {
        history
            .as_array()
            .expect("a list of the talk")
            .iter()
            .filter(|message| {
                !message["content"]
                    .as_str()
                    .unwrap_or_default()
                    .starts_with(DATE_HEAD)
            })
            .cloned()
            .collect()
    }

    // The lines of the day and the clock time of a talk, in the order they stand.
    fn date_lines(history: &Value) -> Vec<String> {
        history
            .as_array()
            .expect("a list of the talk")
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
        // One line of the day, at the head of the talk, of the role `system`.
        let lines = date_lines(&history);
        assert_eq!(lines.len(), 1, "one line of the day: {:?}", lines);
        let items = history.as_array().expect("a list of the talk");
        assert_eq!(items[0]["role"], json!("system"));
        assert!(items[0]["content"]
            .as_str()
            .expect("a line of the day first")
            .starts_with(DATE_HEAD));
        // The words of the reader go out as they were written: the line is a
        // message of its own and not a word added to them.
        assert_eq!(items[1]["content"], json!("the first prompt"));
        let said = undated(&history);
        assert_eq!(said.len(), 5, "the line is not one of the words of the chat");
        for message in &said {
            // No message of the reader carries the line, and none of them is
            // given out as a saying of the program either.
            assert_ne!(message["role"], json!("system"));
            assert!(
                !message["content"]
                    .as_str()
                    .unwrap_or_default()
                    .starts_with(DATE_HEAD),
                "a message of the reader carries the line: {:?}",
                message
            );
        }
    }

    // The line names the moment the talk is made up, and says out loud that it
    // is the latest this machine has. A chat that runs on into the next day is
    // then told a true day, instead of the day its first message went out.
    #[test]
    fn the_day_of_the_talk_is_the_latest_of_this_machine() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        let mut state = engine.state.lock().unwrap();
        dated_chat(&mut state, &Config::default());
        let lines = date_lines(&state.build_history());
        assert_eq!(lines.len(), 1);
        // The day of the week and the day of the month, which are what the line
        // is looked at against. The hour moves a minute at a time.
        let today = store::format_date_clock(store::now_secs());
        let parts: Vec<&str> = today.split(", ").take(2).collect();
        let day = parts.join(", ");
        assert!(lines[0].contains(&day), "the day of this machine: {:?}", lines[0]);
        assert!(lines[0].contains("latest"), "the latest: {:?}", lines[0]);
        assert!(lines[0].contains("latest the machine has"), "the hour of this machine: {:?}", lines[0]);
        // The line is a note and not a question: a model that reads it as a thing
        // to answer writes to itself that no real query stands there, and those
        // words come out as thinking before an answer.
        assert!(lines[0].contains("not a question"));
        assert!(lines[0].contains("do not speak of it"));
    }

    #[test]
    fn the_day_can_stand_away_altogether() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        let mut state = engine.state.lock().unwrap();
        let mut cfg = Config::default();
        cfg.chat_date_new_chat = false;
        dated_chat(&mut state, &cfg);
        let history = state.build_history();
        assert!(date_lines(&history).is_empty());
        assert_eq!(history.as_array().expect("a list of the talk").len(), 5);
    }

    // The rule of the one line: the switch on gives a message of the role
    // `system`, and the switch off gives nothing at all. The line never stands
    // beside a message of the reader, so it can never stand twice.
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

    // The head of the talk must not move while the talk runs. The line of the day
    // is written from the moment the chat was begun and from no other, so every
    // turn of a chat - above all the turn that carries the answer of a tool, which
    // comes back seconds after the call was made - goes out with the head the
    // server has already worked through, and the prompt cache holds. A line written
    // from the clock of the moment made a new head every minute, and the whole talk
    // was read over again on every turn with the caching switch on.
    #[test]
    fn the_head_of_the_talk_does_not_move_between_turns() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        let mut state = engine.state.lock().unwrap();
        dated_chat(&mut state, &Config::default());
        // The head as it stands now, and as it stands a minute later, and as it
        // stands after a run of a tool came in among the talk.
        let first = date_lines(&state.build_history());
        let began = state.chat().expect("a chat stands open").meta.created;
        let said = date_message(&Config::default(), began).expect("the switch stands on");
        assert_eq!(first.len(), 1);
        assert_eq!(said["content"].as_str().expect("the words of the line"), first[0]);
        // A minute of the clock, an hour of it, and a day of it make no difference
        // to the head of a chat that has begun.
        for later in [60i64, 3_600, 86_400, 604_800] {
            let moved = date_message(&Config::default(), began + later)
                .expect("the switch stands on");
            assert_ne!(
                moved["content"].as_str().expect("the words of the line"),
                first[0],
                "a moment other than the beginning of the chat makes a new head"
            );
        }
        // The rule that matters to the server: two looks at the same chat give one
        // and the same head, whatever the clock says between them.
        let again = date_lines(&state.build_history());
        assert_eq!(again, first, "the head of the talk moved between two looks");
        // And the head is the first message of the talk, which is the part the
        // server looks at first when it looks for work it has done before.
        let list = state.build_history();
        let head = list.as_array().expect("a list of the talk")[0].clone();
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
        // The call goes back as the words of the turn it stood in. A server set
        // to read arguments out of a call would fault over words that stop in the
        // middle of a string, which is the very fault the summarisation is meant
        // to keep the talk out of.
        assert_eq!(items[1]["role"], json!("assistant"));
        assert!(items[1].get("tool_calls").is_none());
        let text = items[1]["content"].as_str().expect("words");
        // The model is told the three things it needs: that the call was
        // cancelled, which tool it was, and what it had written when it stopped.
        assert!(text.contains("cancelled"));
        assert!(text.contains(editfile::EDIT_NAME));
        assert!(text.contains("the pl"), "the words so far were lost");
        assert!(text.contains("again"), "the model was not told to write it over");
        // The words stop in the middle, and the model hears so: the string it was
        // writing was never shut.
        assert!(text.contains("ends in the middle"));
        // And nothing of it is a call, nor a receipt of one: the talk that goes
        // out has no tool entry of any kind in it.
        let told = json!(items).to_string();
        assert!(!told.contains("tool_calls"));
        assert!(!told.contains("tool_call_id"));
        // The reader sees the block of the call, named for the file the model was
        // writing into, with the word of what became of it.
        drop(state);
        let snap = engine.snapshot();
        let last = snap.messages.last().expect("a message stands last");
        assert!(last.cancelled);
        assert!(last.failed);
        assert_eq!(last.role, "Tool");
        assert_eq!(last.body, "notes.md");
        assert_eq!(last.tool, CANCELLED_WORDS);
        // The whole of the words of the call rides on the message, so the model
        // is given what it wrote and not a summary of it.
        assert_eq!(last.tool_args, cut.arguments);
    }

    // A call the reader stopped with their own hand. The block stays in the chat,
    // for the reader stopped a thing they could see being written, and the model is
    // told the reader stopped it. Nothing of the room stands in the telling, for
    // the room was never come to and no room was made.
    #[test]
    fn a_call_stopped_by_the_reader_stands_in_the_chat_and_goes_back_as_words() {
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
        state.push_stopped_by_reader(&stopped, "the thinking of the turn", 120, GREATEST_CUT_ARGS);
        let history = state.build_history();
        // The line of the day and the clock time stands at the head of the talk,
        // so the words of the talk are counted without it.
        let items = undated(&history);
        // The call goes back as the words of the turn it stood in, as a call the
        // room gave out under does: a server set to read arguments out of words
        // that stop in the middle of a string would fault over them.
        assert_eq!(items[1]["role"], json!("assistant"));
        assert!(items[1].get("tool_calls").is_none());
        let text = items[1]["content"].as_str().expect("words");
        assert!(text.contains("cancelled"));
        // The reader is named as the hand that stopped it, and nothing of a room
        // is told, for the room was not come to.
        assert!(text.contains("reader stopped"), "the reader was not named: {}", text);
        assert!(!text.contains("summarisation"), "a room was told of: {}", text);
        assert!(!text.contains("handoff"), "a room was told of: {}", text);
        assert!(text.contains(editfile::EDIT_NAME));
        assert!(text.contains("the pl"), "the words so far were lost");
        assert!(text.contains("ends in the middle"));
        assert!(!history.to_string().contains("tool_calls"));
        assert!(!history.to_string().contains("tool_call_id"));
        // The reader sees the block of the call, named for the file the model was
        // writing into, with the word of what became of it: their own stop, and not
        // a fault of the tool.
        drop(state);
        let snap = engine.snapshot();
        let last = snap.messages.last().expect("a message stands last");
        assert_eq!(last.role, "Tool");
        assert!(last.cancelled);
        assert!(last.reader_stopped);
        assert!(last.failed);
        assert_eq!(last.body, "notes.md");
        assert_eq!(last.tool, STOPPED_WORDS);
        // A call the room gave out under keeps its own words, and does not borrow
        // the stop of the reader. The two hands cannot be mistaken for one another
        // in the chat, nor in what goes back to the model.
        let mut state = engine.state.lock().unwrap();
        let room = cut_call(tools::TOOL_NAME, "{\"command\":\"ls -");
        state.push_cancelled(&room, "", 0, GREATEST_CUT_ARGS);
        drop(state);
        let snap = engine.snapshot();
        let last = snap.messages.last().expect("a message stands last");
        assert!(last.cancelled);
        assert!(!last.reader_stopped);
        assert_eq!(last.tool, CANCELLED_WORDS);
        let told = stopped_words(editfile::EDIT_NAME, "{\"path\":\"a");
        let room_told = cut_words(editfile::EDIT_NAME, "{\"path\":\"a");
        assert!(told.contains("reader stopped") && !room_told.contains("reader stopped"));
        assert!(room_told.contains("summarisation") && !told.contains("summarisation"));
    }

    // The meter of the context is read again when the server gave no number at the
    // end of an answer. An answer the reader stopped leaves the words the model had
    // written out of the talk again, so the live number of that answer is the
    // number of a talk that no longer exists, and a meter that held it could never
    // climb away from it again.
    #[test]
    fn the_meter_is_read_again_when_no_number_came_back() {
        let talk = json!([
            { "role": "user", "content": "the first prompt" },
            { "role": "assistant", "content": "the answer of it" },
        ]);
        // The server spoke the number of the whole answer: that stands.
        assert_eq!(used_after(4321, &talk), 4321);
        // Nothing came back, so the talk that stands in the chat is measured.
        assert_eq!(used_after(0, &talk), estimate_tokens(&talk));

        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        let mut state = engine.state.lock().unwrap();
        state.start_chat();
        add_message(&mut state, "You", "the first prompt");
        // The meter climbed while the model wrote a call, as it does. The reader
        // stopped, and the words the number counted are not in the chat.
        state.ctx_used = 900_000;
        let told = state.build_history();
        state.ctx_used = used_after(0, &told);
        assert_eq!(
            state.ctx_used,
            estimate_tokens(&told),
            "the number of a talk that is gone was kept over the chat"
        );
    }

    // The first number of the talk that a new ask brings is taken where it stands.
    // After that the meter only climbs, as it always did.
    #[test]
    fn the_meter_of_a_new_ask_is_taken_where_it_stands() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        open_a_chat(&engine);
        let mut state = engine.state.lock().unwrap();
        // A number left over from an answer the reader stopped: the words it
        // counted went out of the talk with the answer.
        state.ctx_used = 5000;
        state.ctx_fresh = true;
        assert!(
            climb_context(&mut state, 1200),
            "the first number of the ask was refused, and the meter would stand still"
        );
        assert_eq!(state.ctx_used, 1200);
        // The rest of the answer climbs as it always did: the words of an answer
        // that is still coming are not taken back.
        assert!(!climb_context(&mut state, 900));
        assert_eq!(state.ctx_used, 1200);
        assert!(climb_context(&mut state, 1500));
        assert_eq!(state.ctx_used, 1500);
        // A number of nothing says nothing of the talk, so it is taken by nobody,
        // and the ask is still fresh for the first number that does say something.
        state.ctx_fresh = true;
        assert!(!climb_context(&mut state, 0));
        assert!(state.ctx_fresh, "a number of nothing spent the freshness");
        assert_eq!(state.ctx_used, 1500);
    }

    // The room the summarisation made is spent on the words of a stopped call as
    // much as on anything else, so what goes back is bounded by what is left over:
    // a telling that filled the room again would have the model stopped a second
    // time before it had a word to give.
    #[test]
    fn the_words_of_a_cut_call_are_kept_to_the_room() {
        // A room nothing has been spent on yet gives the most a telling is ever
        // given, and a room nearly full keeps the least -- but never nothing, for
        // the head of a call is what names the work it was.
        assert_eq!(cut_bound(0, 8192), GREATEST_CUT_ARGS);
        assert_eq!(cut_bound(8100, 8192), LEAST_CUT_ARGS);
        // In between, a tenth of the room is held back for the answer the model is
        // to write, and what is left over is turned into characters at the four
        // characters to a token the estimate of a talk goes by.
        assert_eq!(cut_bound(6873, 8192), 2000);
        // Nothing is known of the room, so the most is given rather than nothing.
        assert_eq!(cut_bound(9000, 0), GREATEST_CUT_ARGS);

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
        // Only the head of the call is kept, and the block the reader is shown is
        // made from the same words, so the two cannot differ about what the model
        // was told. The head still names the file.
        assert_eq!(last.tool_args.chars().count(), 600);
        assert!(last.tool_args.starts_with("{\"path\":\"notes/big.md\""));
        assert_ne!(last.tool_args, arguments);
        assert_eq!(last.body, "notes/big.md");
        // And the words the model is given say that the call was writing, and
        // name the file the call was writing into. The words of the call are
        // kept to the head of it, for the head is the work the call was, and
        // the whole of the file is the work the summary already stands for.
        let told = history.to_string();
        assert!(told.contains("writing"), "the model was not told the call was writing");
        assert!(told.contains("notes/big.md"), "the head of the call did not name the file");
        // The last of the words the model had written is not there: only the head
        // of the file it was writing out went back with the telling.
        let last_part = format!("part-{:0>3}-", 3 * (GREATEST_CUT_ARGS / 400 + 4) - 1);
        assert!(arguments.contains(&last_part));
        assert!(!told.contains(&last_part), "the whole of the file went back");
        // A call that shut all its marks is whole as far as anyone can tell, and
        // is not said to stop anywhere -- though it is still said never to have
        // run, which is the thing that matters.
        let whole = cut_words(editfile::CREATE_NAME, "{\"path\":\"small.md\"}");
        assert!(!whole.contains("ends in the middle"));
        assert!(whole.contains("never ran"));
        // A call stopped before it had a name still says what became of it.
        let nameless = cut_words("", "");
        assert!(nameless.contains("cancelled"));
        assert!(nameless.contains("the model had not written the name of the tool yet"));
    }

    // The room a summarisation made is weighed on the words the next ask really
    // carries. The telling of a call that was stopped where the model wrote it is
    // the longest message of such a chat, for it holds the words of the call; and
    // the ask for a summary is written of words alone, so that telling stands out of
    // it. A chat that is past the limit only because of the telling therefore has
    // room enough left to make a second summary with, and the answer is not stopped
    // under the block of a call that never ran.
    #[test]
    fn the_room_is_weighed_without_the_telling_of_the_call() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        let mut state = engine.state.lock().unwrap();
        state.start_chat();
        add_message(&mut state, "You", "the first prompt");
        // A call of a great many words, and the telling of it is a long message.
        let mut arguments = String::from("{\"path\":\"notes/big.md\",\"content\":\"");
        for part in 0..40 {
            arguments.push_str(&format!("part-{:0>3}-", part % 100));
            arguments.push_str(&"y".repeat(391));
        }
        let long = cut_call(editfile::CREATE_NAME, &arguments);
        state.push_cancelled(&long, "", 0, GREATEST_CUT_ARGS);
        let whole = estimate_tokens(&state.build_history());
        let asked = carrying_the_cut(&state.build_history());
        // The telling is out of the words the next summary ask carries. The
        // telling stands in the place of the turn that was cut, and the turn
        // that was cut is the words of the answer and the tool block of it, so
        // the telling is the words of the turn, and the room the talk takes with
        // it is the room the turn took. A talk with a telling in it is bigger
        // than the same talk without it.
        assert!(
            whole > asked,
            "the telling was not left out: {} of {}",
            whole,
            asked
        );
        // And the ask for a summary carries the talk as it stands, so the telling
        // of the call that never ran stands in it as it stands in the chat.
        let said = summary_ask(&state.build_history(), "Summarise it.", None).to_string();
        assert!(
            said.contains(CUT_TOLD_FIRST),
            "the telling of the cut call did not go to the ask"
        );
        // The telling is kept to the head of the call, which is the part worth the
        // room: a whole file in the arguments of a cut call is the work the
        // summary already stands for.
        assert!(!said.contains(&"y".repeat(2000)), "the whole call went to the summary ask");
    }

    // The words that stand before a summarisation go back to the model no more.
    // So the call that was stopped has to be written down after the mark of the
    // summarisation, or the model would be left with no word of what it was put
    // to -- and the room it was stopped at would be spent again on the next ask.
    #[test]
    fn a_cut_call_is_written_after_the_summarisation_so_the_model_reads_it() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        let mut state = engine.state.lock().unwrap();
        state.start_chat();
        add_message(&mut state, "You", "the first prompt");
        add_message(&mut state, "Assistant", "a long answer, and then some more of it");
        // A call written down before the summarisation stands with the words the
        // summary stands for, and is left out of the prompt.
        let early = cut_call(tools::TOOL_NAME, "{\"command\":\"cargo test ");
        state.push_cancelled(&early, "", 0, GREATEST_CUT_ARGS);
        let mark = state.begin_summary();
        state.finish_summary(&mark, "Done: the cut. Next: the words.", 7000, 88, "");
        let late = cut_call(editfile::EDIT_NAME, "{\"path\":\"notes.md\",\"edits\":[{\"old_string\":\"the pl");
        state.push_cancelled(&late, "", 0, GREATEST_CUT_ARGS);
        let history = state.build_history();
        let told = history.to_string();
        assert!(!told.contains("cargo test"), "a call before the summary came back");
        // The call that was stopped after the summarisation comes back, and the
        // model is told what it had been writing. The head of the call names the
        // work it was, and the words of the call are kept to the head: the whole
        // of the file is the work the summary already stands for, and sending it
        // again would spend the room the summarisation just made on words the
        // model has read in the summary already.
        assert!(told.contains(editfile::EDIT_NAME), "the tool the call was for was not named");
        assert!(told.contains("the pl"), "the head of the call was lost");
        assert!(told.contains("writing"), "the model was not told what it had been writing");
        assert!(told.contains("cancelled"));
        // It stands after the summary, where the model reads it.
        let items = history.as_array().expect("a list of the talk");
        let where_summary = items
            .iter()
            .position(|item| {
                item["content"]
                    .as_str()
                    .unwrap_or("")
                    .contains("The session was compacted")
            })
            .expect("the summary stands in the prompt");
        let where_cut = items
            .iter()
            .rposition(|item| {
                item["content"]
                    .as_str()
                    .unwrap_or("")
                    .contains("cancelled")
            })
            .expect("the cut call stands in the prompt");
        assert!(where_cut > where_summary);
    }

    // A turn may have put words into the chat before the mark of the
    // summarisation was put in: the words of the answer that was cut, and the
    // thinking of that turn. They are the words the summary stands for, and the
    // mark is carried to the front of them, so the chat stands as the model
    // reads it: the first prompt, the summary, and the words that came after
    // it.
    #[test]
    fn the_words_of_a_cut_turn_stand_before_the_mark_of_the_summarisation() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        let mut state = engine.state.lock().unwrap();
        state.start_chat();
        add_message(&mut state, "You", "the first prompt");
        add_message(&mut state, "Assistant", "the old answer");
        // The answer that was cut at the limit put words and a tool block into
        // the chat, and the mark of the summarisation went in after them, for
        // the mark goes in before the ask and the cut was made before the mark
        // was put in.
        add_message(&mut state, "Assistant", "the words of the cut answer");
        let cut = cut_call(tools::TOOL_NAME, "{\"command\":\"ls");
        state.push_cancelled(&cut, "the thinking of the turn", 0, GREATEST_CUT_ARGS);
        let mark = state.begin_summary();
        state.finish_summary(&mark, "Done: the parser. Next: the bar.", 7000, 88, "");
        // The mark stands before the words of the cut turn, so the reader sees
        // the cut where it was made, and the thinking of the turn does not
        // stand after the heading of the summarisation.
        let chat = state.chat().expect("a chat stands open");
        let where_mark = chat
            .messages
            .iter()
            .position(|m| m.id == mark)
            .expect("the mark stands in the chat");
        let where_cut = chat
            .messages
            .iter()
            .position(|m| m.role == "Tool" && m.cancelled)
            .expect("the words of the cut turn stand in the chat");
        assert!(
            where_cut < where_mark,
            "the mark stood before the words it cuts: mark at {} and the words at {}",
            where_mark, where_cut
        );
        // The words of the cut turn stand for the summary, so they are not sent
        // to the model again: the model reads the first prompt and the summary,
        // and the words that came after the mark.
        add_message(&mut state, "You", "and the next thing");
        let items = undated(&state.build_history());
        let where_first = items
            .iter()
            .position(|item| item["content"].as_str() == Some("the first prompt"))
            .expect("the first prompt stands in the prompt");
        let where_summary = items
            .iter()
            .position(|item| {
                item["content"]
                    .as_str()
                    .unwrap_or("")
                    .starts_with(HANDOFF_FIRST)
            })
            .expect("the summary stands in the prompt");
        let where_next = items
            .iter()
            .position(|item| item["content"].as_str() == Some("and the next thing"))
            .expect("the words after the mark stand in the prompt");
        assert!(where_first < where_summary, "the summary stood before the first prompt");
        assert!(where_summary < where_next, "the words after the mark stood before the summary");
        let said = json!(items).to_string();
        assert!(
            !said.contains("the words of the cut answer"),
            "the words the summary stands for went back to the model"
        );
        assert!(
            !said.contains("the thinking of the turn"),
            "the thinking the summary stands for went back to the model"
        );
    }

    // A mark of a summarisation that was never written stands in the chat with
    // nothing in it. The words before it are the words that mark stands for, and
    // a second summary of them is a summary of a summary. The ask for the summary
    // is made of the words that stand after the mark, and the mark is not sent to
    // the model with the ask.
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
        // The words after the mark: a turn that came after the mark was put in.
        add_message(&mut state, "You", "and the next thing");
        add_message(&mut state, "Assistant", "the next answer");
        // The ask for the summary is made of the words that stand after the
        // mark, and the mark is not sent to the model with the ask.
        let asked = summary_ask(&state.build_history(), "Summarise it.", None);
        let said = asked.to_string();
        // The words after the mark are sent with the ask, for they are the words
        // the summary is written of.
        assert!(said.contains("and the next thing"), "the words after the mark did not go to the ask");
        assert!(said.contains("the next answer"), "the words after the mark did not go to the ask");
        // The mark is not sent to the model with the ask, for it stands for the
        // words before it, and a summary of the mark is a summary of a summary.
        assert!(
            !said.contains(HANDOFF_FIRST),
            "the mark of a summary that was never written went to the ask"
        );
    }

    // The room that was made is weighed on the words the next ask really
    // carries, with the telling of the stopped call left out of it. The telling
    // is the longest message of a chat that came to the limit in the middle of
    // a call, and the estimate of the words reads them bigger than the server
    // counts them. A chat that is past the limit only because of the telling
    // therefore has room enough left to make a second summary with, and the
    // answer is not stopped under the block of a call that never ran.
            #[test]
    fn the_answer_goes_on_after_a_summary_that_the_estimate_reads_full() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        let mut state = engine.state.lock().unwrap();
        state.start_chat();
        add_message(&mut state, "You", "the first prompt");
        // A call of many words, and the telling of it is a long message. The
        // words of the call are kept to the head of it, so the telling is the
        // head, and the head is what the summary already stands for.
        let mut arguments = String::from("{\"path\":\"notes/big.md\",\"content\":\"");
        for part in 0..40 {
            arguments.push_str(&format!("part-{:0>3}-", part % 100));
            arguments.push_str(&"y".repeat(391));
        }
        let long = cut_call(editfile::CREATE_NAME, &arguments);
        state.push_cancelled(&long, "", 0, GREATEST_CUT_ARGS);
        // The telling of the call is kept to the head of it, for the head is the
        // work the call was, and the words of the call are the work the summary
        // already stands for. The head of a call that writes a file is the file,
        // and the file is the part worth the room.
        let told = cut_words(editfile::CREATE_NAME, &arguments);
        let head_of_the_call = editfile::file_of(&arguments);
        assert!(
            !head_of_the_call.trim().is_empty(),
            "the head of the call named nothing: {}",
            head_of_the_call
        );
        assert!(
            told.contains(&head_of_the_call),
            "the telling did not name the file the call was writing: {}",
            told
        );
        assert!(
            told.chars().count() < arguments.chars().count() / 2,
            "the telling held the whole of the words of the call: {} of {}",
            told.chars().count(),
            arguments.chars().count()
        );
        // The room of the model, and the limit of the summarisation, set so that
        // the talk came to the limit while the call was being written. The room
        // is small enough that the estimate of the talk, with the telling of the
        // call in it, reads past the limit, and the estimate of the same talk
        // without the telling reads under it.
        state.ctx_total = 2048;
        state.ctx_used = 1600;
        let mut cfg = Config::default();
        cfg.chat_compact_by = config::COMPACT_BY_TOKENS.to_string();
        cfg.chat_compact_reserve = 1500;
        state.cfg = cfg;
        // The room is made: the summary is written into the mark, and the meter
        // is read again from the talk as it stands.
        let mark = state.begin_summary();
        state.finish_summary(&mark, "Done: the parser. Next: the bar.", 1600, 250, "");
        let made = estimate_tokens(&state.build_history());
        state.ctx_used = made;
        // The words the summary stands for are left out of the talk, so the room
        // the talk takes is the room the first prompt and the summary take, and
        // the telling of the call is no part of it. The estimate of the talk is
        // the number the meter reads, and the number is under the limit, for the
        // room was made.
        let whole = estimate_tokens(&state.build_history());
        assert!(
            !config::compaction_wanted(&state.cfg, whole, state.ctx_total),
            "the room was not made: {} of {}",
            whole,
            state.ctx_total
        );
        // The room is weighed on the words the next ask really carries, and the
        // answer goes on after the summary whatever the estimate says of the
        // whole of the talk: a summary that was kept is room that was made. The
        // first cut of an answer goes on after the room is made, whatever the
        // estimate says of the whole of the talk.
        let still_full = config::compaction_wanted(&state.cfg, whole, state.ctx_total);
        let cuts = 1usize;
        let goes_on = still_full && cuts > 1;
        assert!(
            !goes_on,
            "the answer was stopped under the block of a call that never ran"
        );
    }

    // The answer goes on after the third summarisation as much as after the
    // first, for as long as each summary frees the room: a task that fills the
    // room again and again is summarised again and again, and the work is not
    // stopped for the fault of the room. The number of the cuts is no roof of
    // its own: only a room that stays past the limit even with the summary in
    // it ends the answer, and it is stopped the second time it is found so.
    #[test]
    fn the_answer_goes_on_after_the_third_summary_that_frees_the_room() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        let mut state = engine.state.lock().unwrap();
        state.start_chat();
        add_message(&mut state, "You", "the first prompt");
        // The room and the limit, set so that the talk came to the limit while
        // the answer was being written, and so that a summary of the talk frees
        // the room: the estimate of the talk with the summary in it reads under
        // the limit.
        state.ctx_total = 2048;
        state.ctx_used = 1600;
        let mut cfg = Config::default();
        cfg.chat_compact_by = config::COMPACT_BY_TOKENS.to_string();
        cfg.chat_compact_reserve = 1500;
        state.cfg = cfg;
        // The summary of the talk frees the room, and the meter is read again
        // from the talk as it stands.
        let mark = state.begin_summary();
        state.finish_summary(&mark, "Done: the parser. Next: the bar.", 1600, 250, "");
        let made = estimate_tokens(&state.build_history());
        state.ctx_used = made;
        let whole = estimate_tokens(&state.build_history());
        assert!(
            !config::compaction_wanted(&state.cfg, whole, state.ctx_total),
            "the room was not freed by the summary: {} of {}",
            whole,
            state.ctx_total
        );
        // The third cut of an answer goes on after the room is made, whatever
        // the number of the cuts: a summary that frees the room is room that was
        // made, and the work is not stopped for the fault of the room.
        let still_full = config::compaction_wanted(&state.cfg, whole, state.ctx_total);
        let cuts = 3usize;
        let room = true;
        let stopped = !room || (still_full && cuts > 1);
        assert!(
            !stopped,
            "the answer was stopped after the third summary that freed the room"
        );
        // A room that stays past the limit even with the summary in it ends the
        // answer the second time it is found so, whatever the number of the cuts
        // before it.
        state.ctx_total = 512;
        let still_full = config::compaction_wanted(&state.cfg, whole, state.ctx_total);
        assert!(
            still_full,
            "the room was not kept past the limit: {} of {}",
            whole,
            state.ctx_total
        );
        let stopped = !room || (still_full && cuts > 1);
        assert!(
            stopped,
            "the answer was not stopped under a room that stayed past the limit"
        );
    }

    // The word that pays the debt of a cut call is a word of the program and not
    // of the reader, so it stands in no message of the chat: the reader never
    // wrote it and has nothing to read where it stands. It rides on the ask
    // alone, and comes to no window. It says nothing of a summarisation, for the
    // handoff block of the summary stands in the talk already, and a second copy
    // of it would cost the room the summarisation was made to win.
    #[test]
    fn the_word_that_pays_the_debt_of_a_cut_call_carries_no_block_of_its_own() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        let mut state = engine.state.lock().unwrap();
        state.start_chat();
        add_message(&mut state, "You", "the first prompt");
        let cut = cut_call(editfile::EDIT_NAME, "{\"path\":\"notes.md\"");
        state.push_cancelled(&cut, "", 0, GREATEST_CUT_ARGS);
        // The word goes out with the ask, and the talk ends on the reader.
        let asked = undated(&go_on_words(&state, CUT_CONTINUE_WORDS));
        let last = asked.last().expect("a talk with the word in it");
        assert_eq!(last["role"], json!("user"));
        assert_eq!(last["content"].as_str().expect("words"), CUT_CONTINUE_WORDS);
        // The word says nothing of a summarisation, for the handoff block of the
        // summary stands in the talk already.
        assert!(
            !CUT_CONTINUE_WORDS.contains(HANDOFF_FIRST),
            "the word carried a block of its own: {}",
            CUT_CONTINUE_WORDS
        );
        // Nothing of it stands in the chat, so nothing of it can come to the
        // window.
        let said = json!(state.snap().messages).to_string();
        assert!(
            !said.contains(CUT_CONTINUE_WORDS),
            "the word of the program came to the window"
        );
    }

    // A chat of four messages: a question, its answer, a second question and its
    // answer. The ids are the ones the page would send back.
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

    // A chat stands open with the messages the reader is looking at, so the marks
    // under a message have a talk to work on.
    fn engine_with(messages: Vec<Message>) -> Engine {
        let _guard = config::test_turn();
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

    // A message of the reader takes the answer it provoked along, for the model
    // answered a talk that no longer reads that way.
    #[test]
    fn a_message_of_the_reader_takes_its_answer_along() {
        let engine = engine_with(four_messages());
        engine.remove_message(String::from("c"));
        let state = engine.state.lock().unwrap();
        let chat = state.chat().expect("the chat stands open");
        let left: Vec<&str> = chat.messages.iter().map(|one| one.id.as_str()).collect();
        assert_eq!(left, vec!["a", "b"], "the answer the message provoked stayed");
    }

    // A message of the model leaves the question behind, for the program does not
    // throw away what a reader wrote behind their back. Everything after the
    // answer goes with it, which is the second question and its answer.
    #[test]
    fn a_message_of_the_model_leaves_the_question_behind() {
        let engine = engine_with(four_messages());
        engine.remove_message(String::from("b"));
        let state = engine.state.lock().unwrap();
        let chat = state.chat().expect("the chat stands open");
        let left: Vec<&str> = chat.messages.iter().map(|one| one.id.as_str()).collect();
        assert_eq!(left, vec!["a"], "the question that led to the answer went with it");
    }

    // A chat left with nothing of the talk is taken away as a chat that was never
    // filled is, and the window goes to the first screen.
    #[test]
    fn a_chat_left_with_nothing_of_the_talk_goes_too() {
        let engine = engine_with(four_messages()[..2].to_vec());
        engine.remove_message(String::from("a"));
        let state = engine.state.lock().unwrap();
        assert!(state.chat().is_none(), "the empty chat is still open");
        assert!(
            state.snap().home,
            "the window is not at the first screen with nothing open"
        );
    }

    // Only an answer the reader stopped may be gone on with. An answer that came
    // to its end of its own, and a message of the reader, are both refused with the
    // words that say so. The answer that carries the mark of the stop passes those
    // guards, and the talk that ends on it is open: the only thing left in the way
    // is the model, which no test has.
    #[test]
    fn only_an_answer_the_reader_stopped_may_be_gone_on_with() {
        let engine = engine_with(four_messages());
        engine.continue_message(String::from("b"));
        // The lock is let go of before the next call, for the calls above take it
        // themselves.
        assert!(
            engine
                .state
                .lock()
                .unwrap()
                .status
                .contains("Only an answer you stopped"),
            "an answer that was never stopped was taken"
        );
        engine.continue_message(String::from("c"));
        assert!(
            engine
                .state
                .lock()
                .unwrap()
                .status
                .contains("Only an answer you stopped"),
            "a message of the reader was taken"
        );
        // The answer under the mark of the stop is known as one that may be gone on
        // with, and its words stand at the end of the talk: the ask is refused for
        // want of a model, and not for want of the mark.
        engine.continue_message(String::from("d"));
        let status = engine.state.lock().unwrap().status.clone();
        assert!(
            status.contains("Choose a model"),
            "the answer with the mark of the stop was refused for the wrong reason: {}",
            status
        );
        let state = engine.state.lock().unwrap();
        assert!(!state.generating, "an answer began with no model to ask");
    }

    // The words to go on from must stand at the end of the talk. A talk that ends
    // on a call, or on a message of the reader, is not open, and nothing is asked
    // of the model there.
    #[test]
    fn an_answer_is_gone_on_from_only_when_the_talk_ends_open() {
        let open = json!([
            { "role": "user", "content": "the question" },
            { "role": "assistant", "content": "the answer, half written" },
        ]);
        assert!(ends_open(&open), "a talk ending on an answer was shut");
        let on_a_call = json!([
            { "role": "user", "content": "the question" },
            { "role": "assistant", "content": null, "tool_calls": [ { "id": "1" } ] },
        ]);
        assert!(!ends_open(&on_a_call), "a talk ending on a call was open");
        let on_a_question = json!([{ "role": "user", "content": "the question" }]);
        assert!(!ends_open(&on_a_question), "a talk ending on a question was open");
        let of_no_words = json!([{ "role": "assistant", "content": "  " }]);
        assert!(!ends_open(&of_no_words), "a talk ending on nothing was open");
    }

    // The mark of an answer that never came to its end stands on one answer at
    // most: the newest one the reader stopped. Going on with it takes the mark off,
    // so a second press cannot go on with the same answer twice.
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
        // A second answer with the mark on it takes the mark off the first.
        state.mark_last_answer_stopped();
        let marked: Vec<String> = state
            .chat()
            .unwrap()
            .messages
            .iter()
            .filter(|one| one.stopped)
            .map(|one| one.id.clone())
            .collect();
        assert_eq!(marked, vec![String::from("d")], "the mark did not move to the newest answer");
        state.clear_stopped_marks();
        assert!(
            !state
                .chat()
                .unwrap()
                .messages
                .iter()
                .any(|one| one.stopped),
            "the mark stayed when the talk came to an end"
        );
    }

    // The name of the file a talk stands in has to be a name the server reads:
    // no mark of a folder, no colon, no question mark, no quote, no character a
    // word is not made of, and never a point at the end. The id of a chat is
    // made of quiet marks today, but a name of a file on the disk can never be
    // trusted to be, for the server throws the whole ask of a talk out over one
    // mark it does not like.
    #[test]
    fn the_name_of_a_talk_is_a_name_the_server_reads() {
        let name = context_name("one/two:three*four?q=\"five|six<seven>eight", "/models/a model.gguf");
        assert!(!name.is_empty(), "a name is never nothing");
        assert!(name.ends_with(".ctx"), "the ending marks the talks of the program");
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

    // The talk of a chat read through by one model is not the talk of the same
    // chat read through by another, so the two stand in two files. The id of a
    // model is a path of the disk, which could never stand in a name at all, so
    // it is hashed in at the end instead.
    #[test]
    fn the_name_of_a_talk_tells_the_models_apart() {
        let one = context_name("chat", "/models/one.gguf");
        let two = context_name("chat", "/models/two.gguf");
        assert_ne!(one, two, "two models keep their own talk of one chat");
        assert_eq!(
            one,
            context_name("chat", "/models/one.gguf"),
            "the name of one chat and one model does not move between runs"
        );
        assert!(!one.contains('/'), "the path of a model is hashed and not written");
    }

    // A chat id longer than the room of a name is not cut at the room of a
    // name: the whole of it goes into a hash, so two chats whose ids run the
    // same way for three hundred marks still stand in two files.
    #[test]
    fn a_long_chat_id_is_hashed_and_still_stands_alone() {
        let long = "a".repeat(300);
        let longer = format!("{}b", long);
        let one = context_name(&long, "/m.gguf");
        let other = context_name(&longer, "/m.gguf");
        assert!(one.len() <= 255, "a long id is brought down to a name that fits");
        assert_ne!(one, other, "the whole of a long id is told apart, not the front of it");
    }

    // A server that is stopped leaves no word of its slots behind. The talks
    // were written out before it went, and the next server has slots of its own
    // that nothing of the one before is understood in.
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
        assert!(state.context_slot.is_empty(), "no chat keeps a slot of a server that is gone");
        assert!(state.context_holds.is_empty(), "and no slot keeps a chat");
    }

    // Two chats and one slot: the chat that comes second takes the slot of the
    // first, and the first is given that same slot again when it comes back, for
    // its talk was written out at the moment the slot was taken. The writing and
    // the reading that go with the changing of a slot are answered quietly when
    // nothing is at the other end of the call, which is how things stand in a
    // test -- and a talk that cannot be kept is a talk read through again, with
    // the same words at the end of it.
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
        // The chat in hand takes the only slot the server has.
        state.lock().unwrap().current = Some(first.clone());
        let slot = context_claim(&state, nowhere, model).expect("a slot for the chat in hand");
        assert_eq!(slot, 0, "the slots are named from nought");
        assert_eq!(
            state.lock().unwrap().context_holds.get(&slot).map(|one| one.as_str()),
            Some(first.as_str()),
            "the slot stands for the chat that took it"
        );
        // The second chat takes that same slot, and the first chat is written
        // down against it no longer -- but not forgotten: it still wants it.
        state.lock().unwrap().current = Some(second.clone());
        let taken = context_claim(&state, nowhere, model).expect("a slot for the chat in hand");
        assert_eq!(taken, slot, "one slot, so both chats stand in it in turn");
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
                "and the slot stands for the chat that is in it now"
            );
        }
        // Back it goes, when the first chat is asked again.
        state.lock().unwrap().current = Some(first.clone());
        let back = context_claim(&state, nowhere, model).expect("the slot of the chat in hand");
        assert_eq!(back, slot, "the chat that came back found the slot it left");
        assert_eq!(
            state.lock().unwrap().context_holds.get(&slot).map(|one| one.as_str()),
            Some(first.as_str()),
            "and it stands in it again"
        );
    }

    // With the keeping of the talks switched off, nothing is claimed: the ask
    // goes out with no slot named, exactly as it did before the keeping was.
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
}
