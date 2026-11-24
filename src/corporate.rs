// Corporate display: one block for an answer, with private reasoning hidden and each assistant
// message shown as its own sub-block. The title follows the current assistant update or tool
// action; completed tool results are summarized in the block and the final assistant message
// remains outside the card. The strings below also let older saved cards keep their shape.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::display;
use crate::download;
use crate::editfile;
use crate::search;
use crate::steps;
use crate::suggest;
use crate::summarise;
use crate::tools;
use crate::viewimage;

// Legacy marker retained to display saved cards created by earlier versions.
pub const NAME: &str = "work_log";

// Legacy marker retained to display saved cards created by earlier versions.
pub const FINISH_NAME: &str = "finish_answer";

// One block of work made by a real tool call. Fields for model-written headings and nested
// progress lines remain so saved cards from earlier versions can still be read.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct WorkBlock {
    // The header of the block, as it is shown.
    pub head: String,
    // The short lines under the header.
    #[serde(default)]
    pub lines: Vec<String>,
    // The tool the block means.
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
    // Nested blocks in legacy saved cards.
    #[serde(default)]
    pub sub: Vec<WorkBlock>,
    // True when this is a progress line from a legacy saved card.
    #[serde(default)]
    pub line: bool,
    // True when the block means several calls of one tool made one after another with nothing
    // of another type between. The calls are inside it, and its header is reported in the
    // plural.
    #[serde(default)]
    pub group: bool,
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

// A short live heading for the first user-visible line of a progress update.
pub fn live_head_from_text(text: &str) -> String {
    let line = text
        .lines()
        .find(|one| !one.trim().is_empty())
        .unwrap_or_default()
        .trim();
    if line.is_empty() {
        String::new()
    } else {
        one_line(String::from(line))
    }
}

// The header of the one block of an answer that came to its end with nothing of the work in it.
pub const DONE: &str = "Done";

// The text of what a call is, while it runs. The corporate header uses these, and so does the
// short block of the call.
fn doing_word(name: &str) -> &'static str {
    match name {
        tools::TOOL_NAME => "Running",
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
        tools::TOOL_NAME => "Ran",
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

// True when a name is a known program tool or a legacy corporate marker.
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

// A real tool call becomes its own work block. Assistant progress messages are stored on their
// assistant messages and rendered as separate sub-blocks by the page.
pub fn begin(blocks: &mut Vec<WorkBlock>, name: &str, id: &str, args: &Value) {
    blocks.push(block_for(name, id, args));
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
            } else if one.tool == tools::TOOL_NAME {
                String::from("Ran a command")
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

// The text the model gets when the corporate display remains. Progress messages become ordered
// sub-blocks, and the final answer remains a normal chat message outside the card.
// Short narration before and after each action is the account the user sees while the model works.
pub fn system_prompt() -> String {
    String::from(
        "Corporate display is on. One progress card belongs to each answer. The user cannot see your private reasoning or tool-call messages. Every assistant progress message outside private reasoning is shown as its own ordered sub-block inside the card. Your final answer is shown once as a normal chat message outside the card. The card title follows your current action; do not repeat progress updates below the card.\n\
         Never use a progress, logging, or finish tool; none is available or needed. Use normal assistant text to keep the user informed. For every task that takes more than one step, begin with a short, concrete sentence saying what you are about to do before you inspect, search, call a tool, edit, run a command, or otherwise act. Do not make the user infer your plan from hidden reasoning or from a tool call.\n\
         After each action, report its useful outcome in a new, brief assistant message before starting the next action. Say what you found, changed, created, tested, or what failed, then name the next step when one remains. Keep each update focused on one action or result; each message becomes its own sub-block, in the order written. Do not combine several unrelated steps into one update, repeat the full request, or send empty status messages.\n\
         Before a substantial investigation or review, give a short user-visible update such as 'I will compare the two implementations before choosing the fix.' After the check, report its useful result. Never expose private chain-of-thought; report only the purpose of the check and its result. Keep the user informed during long work with concise updates when the current action changes or a meaningful result arrives.\n\
         Before a tool call, tell the user what the call will do. After its result, explain the relevant outcome before making another call. Do this for each call, including consecutive calls. For file changes, identify the files or behavior changed and mention relevant checks and their outcome. Be accurate: do not claim a check passed or an action succeeded unless it did.\n\
         When the work is complete, write one clear final assistant message. It appears outside the card as the normal chat response. Summarize the result and any important remaining limitation there; do not repeat it in a progress sub-block or use a special tool or marker.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
        assert_eq!(made[1].head, "Ran ls");
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
