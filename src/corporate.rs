// Corporate display: one block for an answer, and the short of the work in it.
//
// The chat can be laid out in two ways. The plain way is the chat as it has
// always stood: the thoughts of the model in their panels, and every call it
// made in a block of its own. The corporate way hides all of that. One block
// stands for an answer, its head saying what the model is at -- Thinking,
// Searching the web, Searched the web, Editing file, Running command -- and
// when the block is opened the work is there, in blocks of its own, each with
// the heading of the area the model is working in and a line or two under it
// saying what it did.
//
// The model writes those lines itself, with a tool of its own. The tool stands
// only while the corporate display is on, and it holds no switch: like the
// tools of planning, it is offered beside the others and cannot be taken away
// by the user, because the mode is nothing without it. The tool has no place
// and no file: what it writes goes into the answer the model is writing, and
// into the message once the answer is kept.
//
// The head of the one block is not written by the model. It comes from what the
// model is doing at the moment, which the program knows better than the model
// does: a search that is on its way is "Searching the web", and a search that
// came back is "Searched the web".

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::display;
use crate::download;
use crate::editfile;
use crate::rerun;
use crate::search;
use crate::steps;
use crate::suggest;
use crate::summarise;
use crate::tools;
use crate::viewimage;

// The name the model uses to write the short of its work.
pub const NAME: &str = "work_log";

// What one call of the tool writes: the heading of the area of work, and the
// short lines to put under it.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Entry {
    #[serde(default)]
    pub topic: String,
    #[serde(default)]
    pub lines: Vec<String>,
}

// One block of the short of the work. A block made by the model carries the
// heading it gave; a block made by the program for a call carries the words of
// the call, so that a search or an edit stands in the short even when the model
// says nothing of it.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct WorkBlock {
    // The head of the block, as it is shown.
    pub head: String,
    // The short lines under the head.
    #[serde(default)]
    pub lines: Vec<String>,
    // The tool the block stands for, empty for a block the model wrote.
    #[serde(default)]
    pub tool: String,
    // The call the block stands for, so that a block is put right and not
    // added twice while an answer comes in.
    #[serde(default)]
    pub id: String,
    // True when the call came back, and true then only when it came back
    // without standing for a fault.
    #[serde(default)]
    pub done: bool,
    #[serde(default)]
    pub failed: bool,
    // What a call that changed a file did to it: lines put in, lines taken out.
    #[serde(default)]
    pub adds: i64,
    #[serde(default)]
    pub cuts: i64,
    // The command a run of the terminal ran. The corporate display hides the
    // block of the call, so this is the only place a user can read what ran.
    #[serde(default)]
    pub command: String,
    // The file, or the files, a call worked on. Empty for a call that worked on
    // nothing a name reaches.
    #[serde(default)]
    pub file: String,
    // The words that came back, short. A fault is read out of these, for no
    // other block of the call stands while the mode is on.
    #[serde(default)]
    pub output: String,
    // The blocks inside this one. A heading the model wrote holds the calls that
    // belong under it, so the short of the work reads as a heading and the work
    // of that heading, and not as one long flat list.
    #[serde(default)]
    pub sub: Vec<WorkBlock>,
}

// The words of the tool the model has. It cannot be switched off, and it is
// offered only while the corporate display stands.
pub fn definition() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": NAME,
            "description": String::from(
                "Write the short of your work into the one block the user sees. The user is reading the corporate display: the thoughts and the tool blocks are hidden, and all of your work shows as a single block whose head says what you are at. This tool puts blocks inside that block. Give the heading of the area, the task or the topic you are working on -- not the tool, and not a file name -- and one to three lines under it, each a very short summary of a thing you did or are doing: 'Comparing the two attention methods.', 'Checking which version the feature needs.' No more than a sentence to a line, and never the answer itself, which the user reads in the chat. Call it again with the same heading to add lines to the same block, and with another heading to open a new one. Call it before a call that takes time, and again when that call comes back, so a user who watches sees the work move: a block nothing has been written into for a minute reads as a program that has stopped. It is not needed for a short answer that asks for nothing, and it is not needed twice for one small thing."
            ),
            "parameters": {
                "type": "object",
                "properties": {
                    "topic": {
                        "type": "string",
                        "description": "The heading of the block: the general area, task or topic of the work these lines belong to. A few words, and the same words for the lines of one piece of work."
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

// The head of the one block while the model is thinking.
pub const THINKING: &str = "Thinking";

// The head of the one block when nothing of the work is going on.
pub const WORKING: &str = "Working";

// The words of what a call is, while it runs. The corporate head uses these,
// and so does the short block of the call.
fn doing_word(name: &str) -> &'static str {
    match name {
        tools::TOOL_NAME => "Running command",
        editfile::EDIT_NAME => "Editing file",
        editfile::CREATE_NAME => "Creating file",
        search::NAME => "Searching the web",
        display::NAME => "Reading the screen",
        suggest::NAME => "Offering a next step",
        steps::NAME => "Writing the steps",
        rerun::NAME => "Running a failed call",
        download::NAME => "Offering files",
        viewimage::NAME => "Looking at a picture",
        summarise::NAME => "Summarising the talk",
        NAME => "Writing the short",
        _ => WORKING,
    }
}

// The same words once the call came back. A search that came back is not
// 'Searching the web'.
fn done_word(name: &str) -> &'static str {
    match name {
        tools::TOOL_NAME => "Ran a command",
        editfile::EDIT_NAME => "Edited a file",
        editfile::CREATE_NAME => "Created a file",
        search::NAME => "Searched the web",
        display::NAME => "Read the screen",
        suggest::NAME => "Offered a next step",
        steps::NAME => "Wrote the steps",
        rerun::NAME => "Ran a failed call",
        download::NAME => "Offered files",
        viewimage::NAME => "Looked at a picture",
        summarise::NAME => "Summarised the talk",
        NAME => "Wrote the short",
        _ => "Worked",
    }
}

// The heading of the area of work the model named in a call that is still being
// written. Nothing stands in the head until the first word of the heading comes
// in. See `crate::tools::partial_text`, which reads a part of a call as far as it
// has been written.
pub fn partial_words(raw: &str) -> String {
    tools::partial_text(raw, "topic")
}

// True when a name is one of the tools of the program, rather than one a
// server of tools added.
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
            | rerun::NAME
            | download::NAME
            | viewimage::NAME
            | summarise::NAME
            | NAME
    )
}

// The head of the one block for a call that is going on. A command carries the
// command, so far as it has been written, for a user who watches a long command
// wants to see which one it is.
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

// The short of a call: the thing it works on, in a few words. Nothing comes
// back for a call that works on no thing.
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

// The first line of a command, for a command the model writes over several
// lines is not shown whole.
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

// The block of a call, as it stands in the short of the work.
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
    }
}

// The command of a call, whole. The head of the block carries only the first
// line of it, for the head is read in a glance; the block carries the whole of
// it, for a command that failed is read from the beginning.
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

// The thing a call worked on: the file it edited, the picture it looked at, or
// the files it offered. Two of a kind are named on one line. A tool that takes
// one name and a tool that takes a list are read the same way here.
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

// Words cut to the room a part of the block has, with a mark that more came
// after them.
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

// A call came back: the head of its block is said in the words of a thing that
// happened, the mark of a fault stands with it, what a call that changed a file
// did to it stands with it, and the words that came back stand inside it. The
// block of the call itself is hidden while the mode stands, so these words are
// the only account of the run the user can read.
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

// Put the words of a call that came back into the head of its block: a search
// that came back searched, and a command that came back ran.
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

// A call began under the short of the work. A heading the model wrote last
// takes the call inside it, so the work of that heading stands with it; with no
// heading open the call stands on its own.
pub fn begin(blocks: &mut Vec<WorkBlock>, name: &str, id: &str, args: &Value) {
    let made = block_for(name, id, args);
    match blocks.last_mut() {
        Some(one) if one.tool == NAME => one.sub.push(made),
        _ => blocks.push(made),
    }
}

// The block of one call, looked for in the whole of the short: a call stands at
// the top, or inside the heading it was made under.
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

// The head of the one block when an answer is over: what the answer did, in the
// words of things that have happened. A search that came back is said as a
// search that was made, and a command that came back as a command that ran.
// Several kinds of work are named one after another, in the order they were
// done; several runs of one kind are named once.
pub fn head_after(blocks: &[WorkBlock]) -> String {
    let mut words: Vec<String> = Vec::new();
    let mut failed = false;
    gather(blocks, &mut words, &mut failed);
    if words.is_empty() {
        return String::from("Done");
    }
    // The three last say what the answer ended with. A head that carried every
    // kind of work would be longer than the answer.
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

// The words of the calls of a short, in the order they were made, with each
// kind named once. `failed` says whether the last call of all came back with a
// fault, which is the fault the head stands for.
fn gather(blocks: &[WorkBlock], words: &mut Vec<String>, failed: &mut bool) {
    for one in blocks.iter() {
        if one.tool.is_empty() {
            gather(&one.sub, words, failed);
            continue;
        }
        if one.tool != NAME {
            let word = if known(&one.tool) {
                String::from(done_word(&one.tool))
            } else {
                one.head.clone()
            };
            if !words.contains(&word) {
                words.push(word);
            }
            *failed = one.failed;
        }
        gather(&one.sub, words, failed);
    }
}

// Take the short of the work out of what the model sent, and put it into the
// short of the answer. A heading that the model wrote before stands open and
// the lines go under it; another heading opens a block of its own. Nothing is
// written for a call that carries no heading and no line.
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
        // Without a heading the lines have nowhere of their own, so they stand
        // under the heading the model opened last, or under a block of their own
        // when the model opened none.
        let place = blocks.last_mut().filter(|one| one.tool == NAME);
        match place {
            Some(one) => one.lines.append(&mut lines),
            None => blocks.push(WorkBlock {
                head: String::from(WORKING),
                lines,
                tool: String::from(NAME),
                ..Default::default()
            }),
        }
        return true;
    }
    // Only the heading at the end of the short takes the lines. A heading the
    // model wrote before, with another one after it, stands as it was: two
    // headings of one name are two blocks of the work, each with the calls that
    // were made under it.
    let place = blocks
        .last_mut()
        .filter(|one| one.tool == NAME && one.head == topic);
    match place {
        Some(one) => one.lines.append(&mut lines),
        None => blocks.push(WorkBlock {
            head: String::from(topic),
            lines,
            tool: String::from(NAME),
            ..Default::default()
        }),
    }
    true
}

// The words the model gets when the corporate display stands. The mode changes
// what the user can see, so the model has to be told how its work reaches the
// screen: the thoughts and the call blocks are hidden, and the short it writes
// is the only account of the work the user reads.
pub fn system_prompt() -> String {
    String::from(
        "The chat stands in the corporate display. The user does not see your thoughts, and does not see the blocks of the calls you make: those are hidden whole. One block stands for each of your answers. Its head says what you are at, and the writing inside it is the short of your work.\n\
         You write that short yourself with work_log. The user reads it while you work, so keep it moving: write to it before a call that takes time, and again when that call comes back. Two calls in a row are two writings, and not one writing after both. A block nothing has been written into for a minute reads as a program that has stopped. A short answer that asks for nothing needs no writing, and one small thing needs no writing of its own.\n\
         Give the heading of the area you are working in, and one to three very short lines under it. Call it again with the same heading to add lines to that heading, and with another heading to open another one; the calls you make go under the heading you opened last. A heading is the area or the task, never a tool name and never a file name.\n\
         When a call comes back, say in one line what it gave you, or what went wrong, so the user learns of a fault as you read it.\n\
         Do not put the answer into those lines. The user reads the answer in the chat, and the short only says what you did to get it.",
    )
}

// The words the model gets back for a call of the tool. They are kept short,
// for the model does not need to be told what it just wrote.
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
        assert_eq!(blocks[0].lines.len(), 2);
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
        assert_eq!(blocks.len(), 1, "the call stands inside the heading");
        assert_eq!(blocks[0].sub.len(), 1);
        assert_eq!(blocks[0].sub[0].head, "Running command ls");
        // The call is found again by its number, inside the heading.
        let found = find_mut(&mut blocks, "c1").expect("the call of the paper");
        settle(found);
        assert_eq!(blocks[0].sub[0].head, "Ran a command ls");
        // A second heading takes the calls after it, and leaves the first one
        // as it stands.
        note(
            &mut blocks,
            &Entry {
                topic: String::from("Writing the answer"),
                lines: vec![String::from("The two methods, side by side.")],
            },
        );
        begin(&mut blocks, search::NAME, "c2", &json!({"query": "attention"}));
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].sub.len(), 1, "the first heading keeps its work");
        assert_eq!(blocks[1].sub.len(), 1);
        // The head of the block names every kind of work the answer did, in the
        // order the kinds were done. The user reads one line and knows the answer
        // both ran a command and searched.
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
                lines: vec![String::from("A thing by itself.")],
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
        assert_eq!(blocks[2].lines.len(), 1);
        assert_eq!(blocks[0].lines.len(), 1);
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
}
