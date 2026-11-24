// Corporate display: one block for an answer, and the summary of the work in it.
//
// The chat can be decompressed in two ways. The plain way is the chat as it has always was: the
// thoughts of the model in their panels, and every call it made in a block of its own. The
// corporate way hides all of that. One block means an answer, its header saying what the model
// is at -- Thinking, Searching the web, Searched the web, Editing file, Running command -- and
// when the block is opened the work is there, in blocks of its own, each with the heading of
// the area the model is working in and a line or two under it saying what it did.
//
// The model writes those lines itself, with a tool of its own. The tool remains only while the
// corporate display is on, and it holds no switch: like the tools of planning, it is proposed
// beside the others and cannot be removed by the user, because the mode is nothing without it.
// The tool has no place and no file: what it writes goes into the answer the model is writing,
// and into the message once the answer is kept.
//
// The header of the one block is not written by the model. It comes from what the model is
// doing at the moment, which the program knows better than the model does: a search that is on
// its way is "Searching the web", and a search that returned is "Searched the web".

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::display;
use crate::download;
use crate::editfile;
use crate::search;
use crate::steps;
use crate::suggest;
use crate::summarise;
use crate::tools;
use crate::viewimage;

// The name the model uses to write the summary of its work.
pub const NAME: &str = "work_log";

// The name the model uses to say that the work is over and its last text come next. The block
// of the answer is closed at that marker, and the text after it are outside it.
pub const FINISH_NAME: &str = "finish_answer";

// What one call of the tool writes: the heading of the area of work, and the short lines to put
// under it.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Entry {
    #[serde(default)]
    pub topic: String,
    #[serde(default)]
    pub lines: Vec<String>,
}

// One block of the summary of the work. A block made by the model carries the heading it gave;
// a block made by the program for a call carries the text of the call, so that a search or an
// edit is in the short even when the model reports nothing of it.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct WorkBlock {
    // The header of the block, as it is shown.
    pub head: String,
    // The short lines under the header.
    #[serde(default)]
    pub lines: Vec<String>,
    // The tool the block means, empty for a block the model wrote.
    #[serde(default)]
    pub tool: String,
    // The call the block means, so that a block is put right and not added twice while an
    // answer arrives.
    #[serde(default)]
    pub id: String,
    // True when the call returned, and true then only when it returned without remaining for an
    // error.
    #[serde(default)]
    pub done: bool,
    #[serde(default)]
    pub failed: bool,
    // What a call that changed a file did to it: lines put in, lines taken out.
    #[serde(default)]
    pub adds: i64,
    #[serde(default)]
    pub cuts: i64,
    // The command a run of the terminal ran. The corporate display hides the block of the call,
    // so this is the only place a user can read what ran.
    #[serde(default)]
    pub command: String,
    // The file, or the files, a call worked on. Empty for a call that worked on nothing a name
    // reaches.
    #[serde(default)]
    pub file: String,
    // The text that returned, short. An error is extracted from these, for no other block of
    // the call remains while the mode is on.
    #[serde(default)]
    pub output: String,
    // The blocks inside this one. A heading the model wrote holds the calls that belong under
    // it, so the summary of the work reads as a heading and the work of that heading, and not
    // as one long flat list.
    #[serde(default)]
    pub sub: Vec<WorkBlock>,
    // True when the block means one line of the summary the model wrote. Such a block holds the
    // calls the model made while that line was last, so the text of the summary and the work
    // they mean are read together. A line with no calls of its own is drawn as a plain line,
    // with nothing to open.
    #[serde(default)]
    pub line: bool,
    // True when the block means several calls of one tool made one after another with nothing
    // of another type between. The calls are inside it, and its header is reported in the
    // plural.
    #[serde(default)]
    pub group: bool,
}

// The text of the tool the model has. It cannot be switched off, and it is proposed only while
// the corporate display remains.
pub fn definition() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": NAME,
            "description": String::from(
                "Update the progress card for this answer. The user cannot see your reasoning or tool calls. Set topic to a short, specific heading for the workstream, and lines to one or two very short updates. Call before every tool call with the action you are starting, and after each result with what happened; do this for consecutive calls too. Reuse topic only to continue that same workstream. When work moves to a different workstream, use a new topic so it becomes a separate heading and preserves the earlier heading. Every line becomes its own progress sub-block, and later updates append rather than replace earlier ones. Keep each line concrete and brief, such as 'Searching for the release notes.' or 'Found the November release date.' Never put the final answer in this card. The card title follows your latest progress while you work; tool blocks show file paths and added or removed line counts. Short direct answers without meaningful work do not need this tool."
            ),
            "parameters": {
                "type": "object",
                "properties": {
                    "topic": {
                        "type": "string",
                        "description": "The heading of the block: the general area, task or topic of the work these lines belong to. A few words, and the same text for the lines of one piece of work."
                    },
                    "lines": {
                        "type": "array",
                        "description": "The short lines to put under the heading: one to three, each a very short summary of what you are doing or have just done.",
                        "items": { "type": "string" }
                    }
                },
                "required": ["topic", "lines"]
            }
        }
    })
}

// The tool that shows the work of the answer is over. It is beside the tool of the summary,
// holds no switch of the user, and is drawn nowhere. What it does is to close the one block of
// the answer: the text the model writes after it is the text the user came for, and it is
// outside the block whole.
pub fn finish_definition() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": FINISH_NAME,
            "description": String::from(
                "Close the work block of this answer and give your last words to the user. Call it once, at the moment you have done everything you meant to do and will call no more tools. The block the user watches stands for your work; this tool ends it, and everything you write afterwards is read as the answer, outside the block, in the plain words of the chat. Do not call it before the work is really over, and do not call a tool after it.",
            ),
            "parameters": {
                "type": "object",
                "properties": {},
            }
        }
    })
}

// True when a name is the tool that closes the block of the answer.
pub fn is_finish(name: &str) -> bool {
    name == FINISH_NAME
}

// The header of the one block while the model is thinking.
pub const THINKING: &str = "Thinking";

// The header of the one block when the model thought and did nothing else. A block that was at
// "Thinking" does not end at "Done", for the thinking is all of what the answer did.
pub fn thought_head(ms: u64) -> String {
    let secs = ((ms as f64) / 1000.0).round() as i64;
    let secs = if secs < 1 { 1 } else { secs };
    if secs == 1 {
        String::from("Thought for 1 second")
    } else {
        format!("Thought for {secs} seconds")
    }
}

// The header of the one block when nothing of the work is going on.
pub const WORKING: &str = "Working";

// The header of the one block of an answer that came to its end with nothing of the work in it.
pub const DONE: &str = "Done";

// The text of what a call is, while it runs. The corporate header uses these, and so does the
// short block of the call.
fn doing_word(name: &str) -> &'static str {
    match name {
        tools::TOOL_NAME => "Running command",
        editfile::EDIT_NAME => "Editing file",
        editfile::CREATE_NAME => "Creating file",
        search::NAME => "Searching the web",
        display::NAME => "Reading the screen",
        suggest::NAME => "Offering a next step",
        steps::NAME => "Writing the steps",
        download::NAME => "Offering files",
        viewimage::NAME => "Looking at a picture",
        summarise::NAME => "Summarising the conversation",
        NAME => "Writing the short",
        _ => WORKING,
    }
}

// The same text once the call returned. A search that returned is not 'Searching the web'.
fn done_word(name: &str) -> &'static str {
    match name {
        tools::TOOL_NAME => "Ran a command",
        editfile::EDIT_NAME => "Edited a file",
        editfile::CREATE_NAME => "Created a file",
        search::NAME => "Searched the web",
        display::NAME => "Read the screen",
        suggest::NAME => "Offered a next step",
        steps::NAME => "Wrote the steps",
        download::NAME => "Offered files",
        viewimage::NAME => "Looked at a picture",
        summarise::NAME => "Summarised the conversation",
        NAME => "Wrote the short",
        _ => "Worked",
    }
}

// The same text in the plural, for several calls of one tool made one after another are in a
// block of their own whose header reports what the answer did, and not what one call did.
fn done_plural(name: &str) -> &'static str {
    match name {
        tools::TOOL_NAME => "Ran commands",
        editfile::EDIT_NAME => "Edited files",
        editfile::CREATE_NAME => "Created files",
        search::NAME => "Searched the web",
        display::NAME => "Read the screen",
        suggest::NAME => "Offered next steps",
        steps::NAME => "Wrote the steps",
        download::NAME => "Offered files",
        viewimage::NAME => "Looked at pictures",
        summarise::NAME => "Summarised the conversation",
        NAME => "Wrote the short",
        _ => "Worked",
    }
}

// The plural text of a run that is still going on.
fn doing_plural(name: &str) -> &'static str {
    match name {
        tools::TOOL_NAME => "Running commands",
        editfile::EDIT_NAME => "Editing files",
        editfile::CREATE_NAME => "Creating files",
        search::NAME => "Searching the web",
        display::NAME => "Reading the screen",
        suggest::NAME => "Offering next steps",
        steps::NAME => "Writing the steps",
        download::NAME => "Offering files",
        viewimage::NAME => "Looking at pictures",
        summarise::NAME => "Summarising the conversation",
        NAME => "Writing the short",
        _ => "Working",
    }
}

// The heading of the area of work the model named in a call that is still being written.
// Nothing is in the header until the first word of the heading arrives. See
// `crate::tools::partial_text`, which reads a part of a call as far as it has been written.
pub fn partial_words(raw: &str) -> String {
    tools::partial_text(raw, "topic")
}

// The latest short line is the most useful header while the model is still working. The topic
// remains a fallback for a call that has not supplied a progress line yet.
pub fn live_head(entry: &Entry) -> String {
    let latest = entry
        .lines
        .iter()
        .rev()
        .map(|one| one.trim())
        .find(|one| !one.is_empty())
        .unwrap_or_else(|| entry.topic.trim());
    if latest.is_empty() {
        return String::from(WORKING);
    }
    one_line(String::from(latest))
}

// True when a name is one of the tools of the program, rather than one a server of tools added.
// The tool that closes the block of the answer is with them, for it is never shown as a block.
fn known(name: &str) -> bool {
    matches!(
        name,
        tools::TOOL_NAME
            | editfile::EDIT_NAME
            | editfile::CREATE_NAME
            | search::NAME
            | display::NAME
            | suggest::NAME
            | steps::NAME
            | download::NAME
            | viewimage::NAME
            | summarise::NAME
            | NAME
            | FINISH_NAME
    )
}

// The header of the one block for a call that is going on. A command carries the command, so
// far as it has been written, for a user who watches a long command wants to see which one it
// is.
pub fn head_of(name: &str, args: &Value) -> String {
    if !known(name) {
        let tool = name.replace('_', " ");
        return if tool.is_empty() {
            String::from(WORKING)
        } else {
            tool
        };
    }
    let word = doing_word(name);
    let what = short_of(name, args);
    if what.is_empty() {
        String::from(word)
    } else {
        format!("{} {}", word, what)
    }
}

// The summary of a call: what it works on, in a few words. Nothing returns for a call that
// works on nothing.
fn short_of(name: &str, args: &Value) -> String {
    let text = |key: &str| -> String {
        args.get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string()
    };
    match name {
        tools::TOOL_NAME => one_line(text("command")),
        editfile::EDIT_NAME | editfile::CREATE_NAME => text("path"),
        search::NAME => one_line(text("query")),
        viewimage::NAME | display::NAME => text("path"),
        _ => String::new(),
    }
}

// The first line of a command, for a command the model writes over several lines is not shown
// whole.
fn one_line(text: String) -> String {
    let line = text.lines().find(|one| !one.trim().is_empty()).unwrap_or_default();
    let line = line.trim();
    const ROOM: usize = 90;
    if line.chars().count() <= ROOM {
        return String::from(line);
    }
    let cut: String = line.chars().take(ROOM).collect();
    format!("{}…", cut.trim_end())
}

// The block of a call, as it is now in the summary of the work.
pub fn block_for(name: &str, id: &str, args: &Value) -> WorkBlock {
    let head = if known(name) {
        let word = doing_word(name);
        let what = short_of(name, args);
        if what.is_empty() {
            String::from(word)
        } else {
            format!("{} {}", word, what)
        }
    } else {
        name.replace('_', " ")
    };
    WorkBlock {
        head,
        lines: Vec::new(),
        tool: String::from(name),
        id: String::from(id),
        done: false,
        failed: false,
        adds: 0,
        cuts: 0,
        command: command_of(name, args),
        file: file_of(name, args),
        output: String::new(),
        sub: Vec::new(),
        line: false,
        group: false,
    }
}

// The command of a call, whole. The header of the block carries only the first line of it, for
// the header is read in a glance; the block carries all of it, for a command that failed is
// read from the beginning.
fn command_of(name: &str, args: &Value) -> String {
    if name != tools::TOOL_NAME {
        return String::new();
    }
    let text = args
        .get("command")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    cut(text, ROOM_COMMAND)
}

// The item a call worked on: the file it edited, the image it checked, or the files it
// proposed. Two of a type are named on one line. A tool that takes one name and a tool that
// takes a list are read the same way here.
fn file_of(name: &str, args: &Value) -> String {
    match name {
        editfile::EDIT_NAME | editfile::CREATE_NAME | viewimage::NAME | display::NAME
        | download::NAME => {
            let mut held: Vec<String> = Vec::new();
            for key in ["path", "paths"] {
                let Some(one) = args.get(key) else { continue };
                let mut add = |word: &str| {
                    let word = word.trim();
                    if !word.is_empty() && !held.iter().any(|was| was == word) {
                        held.push(String::from(word));
                    }
                };
                match one {
                    Value::String(text) => add(text),
                    Value::Array(list) => {
                        for part in list.iter().filter_map(Value::as_str) {
                            add(part);
                        }
                    }
                    _ => {}
                }
            }
            cut(&held.join(", "), ROOM_FILE)
        }
        _ => String::new(),
    }
}

// Text cut to the space a part of the block has, with a marker that more text came after it.
fn cut(text: &str, room: usize) -> String {
    let count = text.chars().count();
    if count <= room {
        return String::from(text);
    }
    let kept: String = text.chars().take(room).collect();
    format!("{}\u{2026}", kept.trim_end())
}

const ROOM_COMMAND: usize = 400;
const ROOM_FILE: usize = 200;
const ROOM_OUTPUT: usize = 1200;

// A call returned: the header of its block is reported as an event, the marker of an error is
// with it, what a call that changed a file did to it is with it, and the text that returned is
// inside it. The block of the call itself is hidden while the mode remains, so this text is the
// only account of the run the user can read.
pub fn came_back(block: &mut WorkBlock, output: &str, ok: bool, adds: i64, cuts: i64) {
    settle(block);
    block.failed = !ok;
    if adds > 0 || cuts > 0 {
        block.adds = adds;
        block.cuts = cuts;
    }
    let text = output.trim();
    if !text.is_empty() {
        block.output = cut(text, ROOM_OUTPUT);
    }
}

// Put the text of a call that returned into the header of its block: a search that returned
// searched, and a command that returned ran.
pub fn settle(block: &mut WorkBlock) {
    if block.done || block.tool.is_empty() {
        return;
    }
    block.done = true;
    if known(&block.tool) {
        let was = doing_word(&block.tool);
        block.head = block.head.replace(was, done_word(&block.tool));
    }
}

// A call began under the summary of the work. A heading the model wrote last takes the call
// inside it, so the work of that heading is with it; with no heading open the call is on its
// own. Inside the heading the call goes under the line the model wrote last, so the text of the
// summary and the work they mean are read together: another line the model writes takes the
// calls that come after it.
pub fn begin(blocks: &mut Vec<WorkBlock>, name: &str, id: &str, args: &Value) {
    let made = block_for(name, id, args);
    match blocks.last_mut() {
        Some(one) if one.tool == NAME => under(one, made),
        _ => blocks.push(made),
    }
}

// The place one call is inside a heading of the summary: under the line the model wrote last,
// or under the heading itself when the heading holds no line of its own.
fn under(head: &mut WorkBlock, made: WorkBlock) {
    match head.sub.last_mut() {
        Some(one) if one.line => one.sub.push(made),
        _ => head.sub.push(made),
    }
}

// The line of the summary the model wrote, made into a block of its own. A line with no call
// under it is drawn as a plain line; the calls that fall under it are what makes it open.
fn line_block(text: String) -> WorkBlock {
    WorkBlock {
        head: text,
        tool: String::from(NAME),
        line: true,
        ..Default::default()
    }
}

// The block of one call, searched for in all of the summary: a call is at the top, or inside
// the heading it was created under.
pub fn find_mut<'a>(blocks: &'a mut [WorkBlock], id: &str) -> Option<&'a mut WorkBlock> {
    for one in blocks.iter_mut() {
        if one.id == id {
            return Some(one);
        }
        if let Some(deeper) = find_mut(&mut one.sub, id) {
            return Some(deeper);
        }
    }
    None
}

// The header of the one block of an answer that came to its end. A block that was at "Thinking"
// does not end at "Done": when the answer did nothing but think, the thinking is all of what it
// did, and is reported with the time it took.
pub fn head_of_answer(blocks: &[WorkBlock], thought_ms: u64) -> String {
    let said = head_after(blocks);
    if said == DONE && thought_ms > 0 {
        return thought_head(thought_ms);
    }
    said
}

// The header of the one block when an answer is over: what the answer did, in the text of items
// that has happened. A search that returned is reported as a search that was created, and a
// command that returned as a command that ran. File calls are counted across the answer.
pub fn head_after(blocks: &[WorkBlock]) -> String {
    let mut words: Vec<String> = Vec::new();
    let mut failed = false;
    let mut file_counts = FileCounts::default();
    count_files(blocks, &mut file_counts);
    gather(blocks, &mut words, &mut failed, &file_counts);
    if words.is_empty() {
        if failed {
            return String::from("Work failed");
        }
        return String::from(DONE);
    }
    // The three last say what the answer ended with. A header that carried every type of work
    // would be longer than the answer.
    const ROOM: usize = 3;
    let from = words.len().saturating_sub(ROOM);
    let mut text = words[from..].join(", ");
    if from > 0 {
        text = format!("\u{2026} {}", text);
    }
    if failed {
        text.push_str(", it failed");
    }
    text
}

// The text of the calls of a short, in the order they were created, with each type named once.
// `failed` reports whether any call returned with an error.
#[derive(Default)]
struct FileCounts {
    edited: usize,
    created: usize,
}

fn count_files(blocks: &[WorkBlock], counts: &mut FileCounts) {
    for one in blocks {
        if !one.id.is_empty() && one.done && !one.failed {
            if one.tool == editfile::EDIT_NAME {
                counts.edited += 1;
            } else if one.tool == editfile::CREATE_NAME {
                counts.created += 1;
            }
        }
        count_files(&one.sub, counts);
    }
}

fn file_count_word(verb: &str, count: usize) -> String {
    format!(
        "{} {} {}",
        verb,
        count,
        if count == 1 { "file" } else { "files" }
    )
}

fn gather(
    blocks: &[WorkBlock],
    words: &mut Vec<String>,
    failed: &mut bool,
    file_counts: &FileCounts,
) {
    for one in blocks.iter() {
        if one.tool.is_empty() {
            gather(&one.sub, words, failed, file_counts);
            continue;
        }
        if one.tool != NAME && one.tool != FINISH_NAME {
            let word = if one.tool == editfile::EDIT_NAME {
                if file_counts.edited == 0 || words.iter().any(|one| one.starts_with("Edited ")) {
                    String::new()
                } else {
                    file_count_word("Edited", file_counts.edited)
                }
            } else if one.tool == editfile::CREATE_NAME {
                if file_counts.created == 0 || words.iter().any(|one| one.starts_with("Created ")) {
                    String::new()
                } else {
                    file_count_word("Created", file_counts.created)
                }
            } else if known(&one.tool) {
                String::from(done_word(&one.tool))
            } else {
                one.head.clone()
            };
            if !word.is_empty() && !words.contains(&word) {
                words.push(word);
            }
            *failed |= one.failed;
        }
        gather(&one.sub, words, failed, file_counts);
    }
}

// The blocks as the user is given them. Calls of one tool made one after another, with nothing
// of another type between, remain together in one block of their own: ten files made one after
// another read as "Created files (10)" with the ten under it, and not as ten blocks of the
// summary. The calls are in the same order, each with its own header, its file and what
// returned. Headings and lines of the summary are not calls, so a run of calls broken by one of
// them is two runs and two blocks.
pub fn grouped(blocks: &[WorkBlock]) -> Vec<WorkBlock> {
    let mut out: Vec<WorkBlock> = Vec::new();
    let mut at = 0;
    while at < blocks.len() {
        let one = &blocks[at];
        // What is inside a heading, or under a line of one, is put together the same way.
        if one.tool == NAME {
            let mut held = one.clone();
            held.sub = grouped(&one.sub);
            out.push(held);
            at += 1;
            continue;
        }
        if !run_of(one) {
            out.push(one.clone());
            at += 1;
            continue;
        }
        let mut run: Vec<WorkBlock> = vec![one.clone()];
        let mut next = at + 1;
        while next < blocks.len() && run_of(&blocks[next]) && blocks[next].tool == one.tool {
            run.push(blocks[next].clone());
            next += 1;
        }
        if run.len() == 1 {
            out.push(one.clone());
        } else {
            out.push(group_of(&run));
        }
        at = next;
    }
    out
}

// True for the block of a call that may be in a run of its type: a call of one tool, and not a
// heading or a line of the summary, and not a block that already means a run.
fn run_of(one: &WorkBlock) -> bool {
    !one.tool.is_empty()
        && one.tool != NAME
        && one.tool != FINISH_NAME
        && !one.line
        && !one.group
}

// The one block that means a run of calls of one tool. Its header is reported in the plural,
// with the number of calls in it, and it is done only when every call of the run returned. An
// error in any of them is an error in the run.
fn group_of(run: &[WorkBlock]) -> WorkBlock {
    let mut done = true;
    let mut failed = false;
    for one in run.iter() {
        if !one.done {
            done = false;
        }
        if one.failed {
            failed = true;
        }
    }
    let word = if done {
        done_plural(&run[0].tool)
    } else {
        doing_plural(&run[0].tool)
    };
    WorkBlock {
        head: format!("{} ({})", word, run.len()),
        tool: run[0].tool.clone(),
        done,
        failed,
        group: true,
        sub: run.to_vec(),
        ..Default::default()
    }
}

// Take the summary of the work out of what the model sent, and put it into the summary of the
// answer. A heading that the model wrote before is left open and the lines go under it; another
// heading opens a block of its own. Nothing is written for a call that carries no heading and
// no line.
pub fn note(blocks: &mut Vec<WorkBlock>, entry: &Entry) -> bool {
    let topic = entry.topic.trim();
    let mut lines: Vec<String> = entry
        .lines
        .iter()
        .map(|one| one.trim())
        .filter(|one| !one.is_empty())
        .map(String::from)
        .collect();
    lines.dedup();
    if topic.is_empty() && lines.is_empty() {
        return false;
    }
    if topic.is_empty() {
        // Without a heading the lines have nowhere of their own, so they are under the heading
        // the model opened last, or under a block of their own when the model opened none.
        let place = blocks
            .last_mut()
            .filter(|one| one.tool == NAME && !one.line);
        match place {
            Some(one) => one.sub.extend(lines.into_iter().map(line_block)),
            None => blocks.push(WorkBlock {
                head: String::from(WORKING),
                sub: lines.into_iter().map(line_block).collect(),
                tool: String::from(NAME),
                ..Default::default()
            }),
        }
        return true;
    }
    // Only the heading at the end of the summary takes the lines. A heading the model wrote
    // before, with another one after it, remains as it was: two headings of one name are two
    // blocks of the work, each with the calls that were created under it.
    let place = blocks
        .last_mut()
        .filter(|one| one.tool == NAME && !one.line && one.head == topic);
    match place {
        Some(one) => one.sub.extend(lines.into_iter().map(line_block)),
        None => blocks.push(WorkBlock {
            head: String::from(topic),
            sub: lines.into_iter().map(line_block).collect(),
            tool: String::from(NAME),
            ..Default::default()
        }),
    }
    true
}

// The text the model gets when the corporate display remains. The mode changes what the user
// can see, so the model has to be informed how its work reaches the screen: the thoughts and
// the call blocks are hidden, and the short it writes is the only account of the work the user
// reads.
pub fn system_prompt() -> String {
    String::from(
        "Corporate display is on. The user cannot see your reasoning or tool-call messages. One progress card belongs to each answer; its title shows your current action, and its contents show short progress headings with their updates and the tool calls made for them. The final assistant reply stays in the chat below the card.\n\
         Use work_log throughout any answer that involves research, tools, file changes, or multiple steps. Before each tool call, write one short progress line saying what you are about to do. After the call returns, write one short line saying what it returned or what failed. Do this for each call, including consecutive calls; do not wait until a group of calls is complete. A short direct answer with no meaningful work needs no progress lines.\n\
         Each work_log call names a workstream in topic and gives one or two concise progress lines in lines. Use a clear, specific heading such as 'Research the release' or 'Create the findings file'. Reuse the same topic only to add progress to that same workstream. Start a new topic when you begin a different workstream; never rename or replace an earlier topic to describe later work. Each topic becomes its own heading block, and each progress line becomes its own sub-block. Keep earlier headings and lines intact.\n\
         Progress lines describe one concrete step or result in a few words. For example: 'Searching the web for the release date.' and then 'Found the November release notes.' Do not repeat the user request, write a generic status, or put your final answer in a progress line. Calls you make after a progress line appear under that line, so write a fresh line before moving to the next step.\n\
         The title of the card follows your latest progress or action while you work. Keep work_log updates frequent enough that it never looks stalled. For file edits, the tool call block also shows the file and added and removed line counts; do not duplicate those counts in progress text.\n\
         When all work and tool calls are complete, call finish_answer exactly once immediately before your final assistant reply. Do not call any tool after finish_answer. The final reply is shown outside the progress card.",
    )
}

// The text the model gets back for the call that closes the block of its answer. They are kept
// short, and they say what comes next.
pub const FINISH_WORDS: &str =
    "The block is closed. What you write now is the answer the user reads.";

// The text the model gets back for a call of the tool. They are kept short, for the model does
// not need to be informed what it just wrote.
pub fn receipt(written: bool) -> String {
    if written {
        String::from("Noted. The user sees it in the block.")
    } else {
        String::from("Nothing was noted: a heading or a line is needed.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_heading_of_the_model_is_kept_and_another_one_opens_a_new_block() {
        let mut blocks = Vec::new();
        assert!(note(
            &mut blocks,
            &Entry {
                topic: String::from("Weighing the two methods"),
                lines: vec![String::from("Comparing the results.")],
            }
        ));
        assert!(note(
            &mut blocks,
            &Entry {
                topic: String::from("Weighing the two methods"),
                lines: vec![String::from("Weighing the caveats.")],
            }
        ));
        assert!(note(
            &mut blocks,
            &Entry {
                topic: String::from("Checking the version"),
                lines: vec![String::from("Which version the feature needs.")],
            }
        ));
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].sub.len(), 2, "the two writings are under one heading");
        assert!(blocks[0].sub[0].line, "a line of the summary is a block of its own");
        assert_eq!(blocks[0].sub[1].head, "Weighing the caveats.");
        assert_eq!(blocks[1].head, "Checking the version");
    }

    #[test]
    fn a_call_stands_under_the_heading_the_model_opened_last() {
        let mut blocks = Vec::new();
        note(
            &mut blocks,
            &Entry {
                topic: String::from("Reading the paper"),
                lines: vec![String::from("Two methods to compare.")],
            },
        );
        begin(&mut blocks, tools::TOOL_NAME, "c1", &json!({"command": "ls"}));
        assert_eq!(blocks.len(), 1, "the call is inside the heading");
        // The line the model wrote remains as the first block of the heading, and the call is
        // under it.
        assert_eq!(blocks[0].sub.len(), 1);
        assert!(blocks[0].sub[0].line);
        assert_eq!(blocks[0].sub[0].head, "Two methods to compare.");
        assert_eq!(blocks[0].sub[0].sub[0].head, "Running command ls");
        // The call is found again by its number, inside the heading and under the line of it.
        let found = find_mut(&mut blocks, "c1").expect("the call of the paper");
        settle(found);
        assert_eq!(blocks[0].sub[0].sub[0].head, "Ran a command ls");
        // A second heading takes the calls after it, and leaves the first one as it is now.
        note(
            &mut blocks,
            &Entry {
                topic: String::from("Writing the answer"),
                lines: vec![String::from("The two methods, side by side.")],
            },
        );
        begin(&mut blocks, search::NAME, "c2", &json!({"query": "attention"}));
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].sub[0].sub.len(), 1, "the first heading keeps its work");
        assert_eq!(blocks[1].sub.len(), 1);
        assert_eq!(blocks[1].sub[0].sub[0].head, "Searching the web attention");
        // The header of the block names every type of work the answer did, in the order the
        // types were done. The user reads one line and knows the answer both ran a command and
        // searched.
        assert_eq!(head_after(&blocks), "Ran a command, Searched the web");
    }

    #[test]
    fn two_headings_of_one_name_are_two_blocks() {
        let mut blocks = Vec::new();
        note(
            &mut blocks,
            &Entry {
                topic: String::from("Work"),
                lines: vec![String::from("The first part.")],
            },
        );
        begin(&mut blocks, tools::TOOL_NAME, "c1", &json!({"command": "pwd"}));
        note(
            &mut blocks,
            &Entry {
                topic: String::from("Elsewhere"),
                lines: vec![String::from("An item by itself.")],
            },
        );
        note(
            &mut blocks,
            &Entry {
                topic: String::from("Work"),
                lines: vec![String::from("The last part.")],
            },
        );
        assert_eq!(blocks.len(), 3, "a heading begun again opens again");
        assert_eq!(blocks[2].sub.len(), 1);
        assert_eq!(blocks[0].sub.len(), 1);
    }

    #[test]
    fn a_call_that_came_back_is_said_in_the_words_of_a_thing_done() {
        let mut block = block_for(search::NAME, "c1", &json!({"query": "attention methods"}));
        assert_eq!(block.head, "Searching the web attention methods");
        settle(&mut block);
        assert_eq!(block.head, "Searched the web attention methods");
        assert!(block.done);
        assert!(!block.failed);
        assert_eq!(head_after(std::slice::from_ref(&block)), "Searched the web");
    }

    #[test]
    fn a_tool_a_server_added_is_called_by_its_own_name() {
        let block = block_for("notion_search", "c2", &json!({}));
        assert_eq!(block.head, "notion search");
        assert_eq!(head_of("notion_search", &json!({})), "notion search");
    }

    // Several calls of one tool made one after another, with nothing of another type between,
    // are in one block whose header is reported in the plural. A call of another tool between
    // them makes two runs and two blocks, and one call of a type is no run at all.
    #[test]
    fn calls_of_one_tool_one_after_another_stand_in_one_block() {
        let mut blocks = Vec::new();
        for at in 0..3 {
            let id = format!("c{at}");
            let path = format!("f{at}.ts");
            begin(&mut blocks, editfile::CREATE_NAME, &id, &json!({"path": path}));
            let found = find_mut(&mut blocks, &id).expect("the file the model made");
            came_back(found, &format!("Created '{path}'."), true, 10, 0);
        }
        begin(&mut blocks, tools::TOOL_NAME, "run", &json!({"command": "ls"}));
        let found = find_mut(&mut blocks, "run").expect("the run");
        settle(found);
        begin(&mut blocks, editfile::CREATE_NAME, "last", &json!({"path": "z.ts"}));
        let made = grouped(&blocks);
        assert_eq!(made.len(), 3, "the run of three, the run between, and the one after");
        assert!(made[0].group);
        assert_eq!(made[0].head, "Created files (3)");
        assert_eq!(made[0].sub.len(), 3);
        assert_eq!(made[0].sub[0].head, "Created a file f0.ts");
        assert_eq!(made[0].sub[0].file, "f0.ts", "a call keeps its own file");
        assert!(made[0].done, "every call of the run returned");
        assert!(!made[1].group, "one run of a kind is no run of its kind");
        assert_eq!(made[1].head, "Ran a command ls");
        assert!(!made[2].group, "one call of a kind is no run");
        // The answer header summarizes all file calls, including the one after another tool.
        assert_eq!(head_after(&made), "Created 3 files, Ran a command");
    }

    #[test]
    fn the_final_heading_counts_created_and_edited_files() {
        let mut blocks = Vec::new();
        for (name, id, path, output) in [
            (
                editfile::CREATE_NAME,
                "c1",
                "new-a.md",
                "Created 'new-a.md'.",
            ),
            (editfile::EDIT_NAME, "e1", "old-a.md", "Edited 'old-a.md'."),
            (
                editfile::CREATE_NAME,
                "c2",
                "new-b.md",
                "Created 'new-b.md'.",
            ),
        ] {
            begin(&mut blocks, name, id, &json!({"path": path}));
            let call = find_mut(&mut blocks, id).expect("the file operation");
            came_back(call, output, true, 1, 0);
        }
        assert_eq!(
            head_after(&grouped(&blocks)),
            "Created 2 files, Edited 1 file"
        );
    }

    #[test]
    fn a_failure_remains_in_the_final_heading_after_later_success() {
        let mut first = block_for(search::NAME, "s1", &json!({"query": "first"}));
        came_back(&mut first, "network error", false, 0, 0);
        let mut second = block_for(tools::TOOL_NAME, "t1", &json!({"command": "pwd"}));
        came_back(&mut second, "/workspace", true, 0, 0);
        assert_eq!(
            head_after(&[first, second]),
            "Searched the web, Ran a command, it failed"
        );
    }

    #[test]
    fn failed_file_operations_are_not_counted_as_changed_files() {
        let mut blocks = Vec::new();
        begin(
            &mut blocks,
            editfile::CREATE_NAME,
            "c1",
            &json!({"path": "already-there.md"}),
        );
        let call = find_mut(&mut blocks, "c1").expect("the file operation");
        came_back(call, "That file already exists.", false, 0, 0);
        assert_eq!(head_after(&grouped(&blocks)), "Work failed");
    }

    // A group is done only when every call of it returned, so a run that is still going on is
    // reported in the plural of the text that is going on.
    #[test]
    fn a_run_that_is_still_going_on_is_said_in_the_words_of_a_thing_doing() {
        let mut blocks = Vec::new();
        begin(&mut blocks, editfile::EDIT_NAME, "e1", &json!({"path": "a.ts"}));
        let found = find_mut(&mut blocks, "e1").expect("the first edit");
        came_back(found, "Edited 'a.ts'.", true, 3, 1);
        begin(&mut blocks, editfile::EDIT_NAME, "e2", &json!({"path": "b.ts"}));
        let made = grouped(&blocks);
        assert_eq!(made.len(), 1);
        assert!(made[0].group);
        assert_eq!(made[0].head, "Editing files (2)");
        assert!(!made[0].done, "one call of the run is still going on");
    }

    // The calls fall under the line the model wrote last. Another line the model writes takes
    // the calls that come after it, so the text of the summary and the work they mean are read
    // together.
    #[test]
    fn a_call_stands_under_the_line_the_model_wrote_last() {
        let mut blocks = Vec::new();
        note(
            &mut blocks,
            &Entry {
                topic: String::from("Build Phase 1"),
                lines: vec![String::from("Creating the schemas.")],
            },
        );
        begin(&mut blocks, editfile::CREATE_NAME, "c1", &json!({"path": "schema.prisma"}));
        note(
            &mut blocks,
            &Entry {
                topic: String::from("Build Phase 1"),
                lines: vec![String::from("Building the APIs.")],
            },
        );
        begin(&mut blocks, editfile::CREATE_NAME, "c2", &json!({"path": "auth.ts"}));
        begin(&mut blocks, editfile::CREATE_NAME, "c3", &json!({"path": "cart.ts"}));
        assert_eq!(blocks.len(), 1, "the heading of the same name is kept");
        assert_eq!(blocks[0].sub.len(), 2, "two lines of the summary are in the heading");
        assert_eq!(blocks[0].sub[0].sub.len(), 1, "the first line holds the call under it");
        assert_eq!(blocks[0].sub[1].sub.len(), 2, "the second line holds the two after it");
        // Put together, the two calls of the second line are in one block under that line, and
        // the first line keeps its one call as it was.
        let made = grouped(&blocks);
        assert_eq!(made[0].sub[0].sub.len(), 1);
        assert_eq!(made[0].sub[1].sub.len(), 1);
        assert_eq!(made[0].sub[1].sub[0].head, "Creating files (2)");
    }

    // The header of an answer that thought and did nothing else reports how long the thinking
    // took, and not that the answer is done. The short the model wrote is no work of a call, so
    // an answer that wrote only the short still reports what it thought.
    #[test]
    fn an_answer_of_thinking_only_says_how_long_it_thought() {
        assert_eq!(thought_head(0), "Thought for 1 second");
        assert_eq!(thought_head(1400), "Thought for 1 second");
        assert_eq!(thought_head(12_400), "Thought for 12 seconds");
        let mut blocks = Vec::new();
        assert_eq!(head_of_answer(&blocks, 0), DONE);
        assert_eq!(head_of_answer(&blocks, 4200), "Thought for 4 seconds");
        note(
            &mut blocks,
            &Entry {
                topic: String::from("Weighing it"),
                lines: vec![String::from("Two ways.")],
            },
        );
        assert_eq!(head_of_answer(&blocks, 4200), "Thought for 4 seconds");
    }

    // The tool that closes the block of the answer is no work of a call: it is named nowhere in
    // the header, and it is in no run.
    #[test]
    fn the_tool_that_closes_the_block_names_no_work() {
        let mut blocks = Vec::new();
        begin(&mut blocks, tools::TOOL_NAME, "c1", &json!({"command": "ls"}));
        let found = find_mut(&mut blocks, "c1").expect("the run");
        settle(found);
        blocks.push(WorkBlock {
            head: String::from("the end of the work"),
            tool: String::from(FINISH_NAME),
            done: true,
            ..Default::default()
        });
        assert_eq!(head_after(&blocks), "Ran a command");
        let made = grouped(&blocks);
        assert_eq!(made.len(), 2, "the closing marker is no call of a run");
    }
}
