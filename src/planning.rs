// Planning mode: the work of laying out a large piece of work before any of it is done.
//
// A user writes `/plan` and the chat changes from a conversation into a drawing board. The
// model is informed, in a prompt of its own, to plan the work down to its smallest part; it has
// one tool for the plan, and no other way of touching it; and when the user reports the plan is
// good, the model calls the second tool, the conversation of the planning is closed, and the
// plan itself becomes the first message of the work.
//
// The plan is in `chats.db`, in the table of the plans, beside the conversation it was written
// for - and not in the working directory of the chat. A plan is therefore never mixed with the work it
// plans, and a command of the terminal tool cannot write over it by accident. Before the
// databases it was in a directory of the data directory under the name `<chat id>.md`; those
// files are read into the table by the scan in `src/migrate.rs`, and the folder goes with them.
//
// The name kept with a chat is still the name of the older keeping, because it is the key the
// row of the plan is looked up by: `plan_file` reports `<chat id>.md`, and the row of the plans
// reports the same id. The tool the model has for the plan carries no place at all: it reaches
// the one file of the one chat, and nothing else. It cannot be switched off by the user, for a
// planning chat without it has nowhere to put its work.
//
// Both tools remain only while planning mode is on. When the plan is let go they go away with
// the planning prompt, so the model that does the work reads neither the one nor the other.
//
// The prompt holds two rules above the rest of it, because the two mistakes a planning model
// makes are the two this mode exists to stop. The first is that the work is not done here: no
// file, no setting, no commit, and no command that leaves an item changed, while reading and
// the commands that only look about stay open, so the plan is still built on what the work is.
// The second is that `end_planning` is the only way out of the mode: a model that shows the
// work has begun has not left the mode, and stays forbidden until the tool is called.

use std::sync::Arc;

use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};

use crate::config;
use crate::db;
use crate::tools::{self, with_rule};

// The name the model uses to change the plan, the name it uses to write all of a plan that has
// nothing in it yet, and the name it uses to say that the plan is done and the work may begin.
pub const EDIT_NAME: &str = "edit_plan";
pub const INIT_NAME: &str = "init_plan";
pub const END_NAME: &str = "end_planning";

// The word a user writes for planning mode, and the text of the same command with its marker
// before it.
pub const COMMAND: &str = "plan";
pub const COMMAND_WORD: &str = "/plan";

// The text of the command, for the list of them the chat composer puts up.
pub const COMMAND_WORDS: &str = "Plan the work before any of it is done. The model writes a plan of the whole piece of work, in the smallest detail, into \
                the plan of this chat, and changes it as you answer. When the \
                plan is agreed, release it, and the model begins the work from \
                the plan.";

// The folder the plans was in, before they were in the database. The scan reads it once. See
// `src/migrate.rs`.
pub fn folder() -> std::path::PathBuf {
    config::paths().data.join("planning")
}

// The database the plans are in: the same file as the conversations, because a plan belongs to
// a chat and is read and put away with it.
fn database() -> Result<Arc<db::Db>, String> {
    db::open(&config::paths().data.join(db::CHATS_DB), db::Kind::Chats)
}

// The name of the plan of one chat. The id of a chat is ten hexadecimal characters, so the name
// needs nothing else to keep it apart.
pub fn file_name(chat: &str) -> String {
    format!("{}.md", neat(chat))
}

// True when a name kept with a chat is one this module could have written. A strange name opens
// nothing at all.
fn looks_like_name(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".md") else {
        return false;
    };
    !stem.is_empty()
        && stem.len() <= 40
        && stem.chars().all(|one| one.is_ascii_hexdigit())
}

// The id of the chat a plan belongs to, from the name it is kept under. Nothing returns for a
// name this module could not have written. The move of the plans into the database reads the
// old folder with it. See `src/migrate.rs`.
pub fn chat_of(name: &str) -> Option<String> {
    if !looks_like_name(name) {
        return None;
    }
    Some(name.strip_suffix(".md")?.to_string())
}

// All of the plan of a chat, out of the table of the plans. Nothing returns for a name this
// module could not have written, or for a chat whose plan is not.
fn read(name: &str) -> Option<String> {
    let chat = chat_of(name)?;
    let database = database().ok()?;
    database
        .read(|conn| {
            Ok(conn
                .query_row(
                    "SELECT text FROM plans WHERE chat_id = ?1",
                    params![chat],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .map_err(db::fault)?)
        })
        .ok()
        .flatten()
}

// Write the whole plan of a chat. The row of the plan is put down whole, so a write that cannot
// be finished leaves the plan that was to be read again.
fn write(name: &str, words: &str) -> Result<(), String> {
    let chat = chat_of(name).ok_or_else(String::new)?;
    let when = crate::store::now_secs();
    let database = database()?;
    database.write(|conn| {
        conn.execute(
            "INSERT INTO plans (chat_id, text, updated) VALUES (?1, ?2, ?3)
             ON CONFLICT(chat_id) DO UPDATE SET text = excluded.text, updated = excluded.updated",
            params![chat, words, when],
        )
        .map_err(db::fault)?;
        Ok(())
    })
}

// True when a chat has a row of the plan at all. A chat that goes into planning mode twice
// keeps the plan it wrote, and a plan that was never written is an item apart from a plan of no
// text.
fn stands(name: &str) -> bool {
    let Some(chat) = chat_of(name) else { return false };
    let Ok(database) = database() else { return false };
    database
        .read(|conn| {
            Ok(conn
                .prepare("SELECT 1 FROM plans WHERE chat_id = ?1")
                .map_err(db::fault)?
                .exists(params![chat])
                .map_err(db::fault)?)
        })
        .unwrap_or(false)
}

// Take the plan of a chat out of the table. A chat that is removed takes its plan with it, so
// the text of a chat that is removed is not in the database for nothing.
pub fn drop_plan(chat: &str) {
    let Ok(database) = database() else { return };
    let gone = database.write(|conn| {
        conn.execute("DELETE FROM plans WHERE chat_id = ?1", params![chat])
            .map_err(db::fault)?;
        Ok(())
    });
    if let Err(problem) = gone {
        crate::proglog::note(
            crate::proglog::STORE,
            &format!("the plan of a chat is not removed: {}", problem),
        );
    }
}

// Make the plan of a chat, and give back the name kept with the chat. A plan that remains
// already is left as it is now, so a chat that goes into planning mode twice does not lose what
// it wrote.
pub fn create(chat: &str) -> String {
    let name = file_name(chat);
    if !stands(&name) {
        // The row is created empty. Text of the program's own in a new plan were text the model
        // then had to scan out of the plan, and text the user read in the window for a plan
        // that was never written. An empty plan is read as not written yet, which is what it
        // is.
        let _ = write(&name, "");
    }
    name
}

// All of a plan, for the popup of the heading and for the first message of the work. Nothing
// returns when the plan is removed.
pub fn words(name: &str) -> Option<String> {
    read(name)
}

// The text of a plan that is not there: stated plainly, so a model is never given a blank space
// for a file it was set to read.
pub const GONE_WORDS: &str = "(this chat has no plan)";

// The greatest size of a plan. A plan longer than this is a text no model can read whole, and a
// call that would grow it past this is refused.
const GREATEST_PLAN_BYTES: usize = 8 * 1024 * 1024;

// The most changes one call may make to the plan.
const MAX_EDITS: usize = 100;

// The call that writes the whole plan at once. It is beside the tool that changes the plan by
// matching text, for a plan that has nothing in it has no text to match: the first writing of a
// plan is a whole of text sent in one call, and every writing after it is a change of the plan
// that remains.
pub fn init_definition() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": INIT_NAME,
            "description": with_rule(String::from(
                "This tool has no path and makes the plan's first writing. Use once, only while the plan is empty; it replaces the entire plan. Use edit_plan for later changes. Provide the complete plan itself, not a promise; never write it with another tool or in your answer.",
            )),
            "parameters": {
                "type": "object",
                "properties": {
                    "plan": {
                        "type": "string",
                        "description": "Complete, self-contained plan text."
                    }
                },
                "required": ["plan"]
            }
        }
    })
}

// Write the whole plan of a chat at once. What was there before is written over, for the call
// is the first writing of a plan and not a change of one: a plan of no text at all is refused,
// so a call that meant nothing never takes a written plan away.
pub fn write_init(name: &str, raw: &str) -> tools::Report {
    let failed = |output: String| tools::Report { ok: false, output };
    if chat_of(name).is_none() {
        return failed(String::from(
            "This chat has no plan, so there is nothing to write.",
        ));
    }
    let Ok(call) = serde_json::from_str::<Value>(raw) else {
        return failed(String::from(
            "The call was not valid JSON, so nothing was written.",
        ));
    };
    let Some(words) = call.get("plan").and_then(|one| one.as_str()) else {
        return failed(String::from(
            "The call carries no text of a plan. Give the whole plan in `plan`.",
        ));
    };
    if words.trim().is_empty() {
        return failed(String::from(
            "Nothing was written. The text you sent holds no plan, and a \
        plan of nothing is no plan.",
        ));
    }
    if words.len() > GREATEST_PLAN_BYTES {
        return failed(String::from(
            "Nothing was written. The plan is longer than the greatest \
        length this program stores.",
        ));
    }
    let before = read(name).unwrap_or_default();
    if let Err(why) = write(name, words) {
        return failed(format!("The plan could not be written: {}", why));
    }
    let mut said = format!(
        "The plan was written whole. It holds {} lines.",
        lines_of(words)
    );
    if !before.trim().is_empty() {
        said.push_str("The plan that was there before this call is \
        replaced.");
    }
    if let Some(head) = head_of(words) {
        said.push_str("\nThe plan begins:\n");
        said.push_str(&head);
    }
    tools::Report { ok: true, output: said }
}

// The call that changes the plan. It carries no path: the plan of this chat is the only file it
// can reach.
pub fn edit_definition() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": EDIT_NAME,
            "description": with_rule(String::from(
                "This tool has no path and edits this chat's plan with exact-text replacements. Copy old_string exactly; edits apply in order and each match must be unique unless replace_all is true. An empty old_string appends text. If any match fails or is ambiguous, nothing changes. This is the only tool for changing an existing plan.",
            )),
            "parameters": {
                "type": "object",
                "properties": {
                    "edits": {
                        "type": "array",
                        "description": "Ordered replacements to apply to the plan.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "old_string": {
                                    "type": "string",
                                    "description": "Exact text to replace; empty appends new_string."
                                },
                                "new_string": {
                                    "type": "string",
                                    "description": "Replacement or appended text."
                                },
                                "replace_all": {
                                    "type": "boolean",
                                    "description": "Replace every match; defaults to false."
                                }
                            },
                            "required": ["old_string", "new_string"]
                        }
                    }
                },
                "required": ["edits"]
            }
        }
    })
}

// The call that shows the plan is done: the user gave leave, and the work may begin. Nothing of
// the planning conversation goes with the work.
pub fn end_definition() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": END_NAME,
            "description": with_rule(String::from(
                "End planning and start the work from the plan. This is the only way planning mode ends. Call only after the user has stated explicit approval of the plan and said to begin; never infer approval from a finished plan. The plan becomes the first message of a fresh conversation, so make it self-contained and do not refer to this chat.",
            )),
            "parameters": {
                "type": "object",
                "properties": {
                    "why": {
                        "type": "string",
                        "description": "Brief quote or summary of the user's explicit approval to begin."
                    }
                },
                "required": []
            }
        }
    })
}

// The prompt that is in the conversation of a chat while planning mode is on. It is sent beside
// the text of the program, in a message of the role `system`, for every ask of a planning chat,
// and it is removed with planning mode.
pub fn system_prompt(plan: &str) -> String {
    format!(
        "PLANNING MODE IS ON\n\n\
         You design the work; you do not execute it. Inspect the project with read-only tools, but do not change work files or run commands that write, test, build, install, format, deploy or alter version control.\n\n\
         The only writable record is this chat's plan ({plan}). Use {init} once, only while the plan is empty; it replaces the whole plan. Use {edit} for later changes and {read} to inspect it. Never write the plan with another tool or in an answer. Keep replies brief and do not repeat the plan.\n\n\
         Write a self-contained plan in the user's language: goal and acceptance criteria; verified starting state and affected paths; constraints; ordered implementation steps; checks and expected outcomes; risks and recovery. Verify details when possible and label unknowns instead of guessing.\n\n\
         The user cannot see the plan. Ask decisions only the user can make through {ask}; keep questions and unresolved choices out of the plan. Wait for answers and record decisions there.\n\n\
         Planning ends only through {end}. Call it only after the user explicitly approves the plan and says to begin. If approval is ambiguous, ask through {ask}. The plan becomes the first message of a fresh conversation, so it must stand alone. A finished plan is not approval.",
        plan = plan,
        init = INIT_NAME,
        edit = EDIT_NAME,
        read = READ_NAME,
        ask = ASK_NAME,
        end = END_NAME,
    )
}

// The command names of the chat composer are kept here, and this is where the next one is
// added. The second of them does not change what the model is told: it puts a message in the
// waiting list of the chat, to send when an answer contains the text the user named. Its text
// is kept with the text of the waiting list, in the engine.
pub const PROGRAM_WORD: &str = "/program";
pub const PROGRAM_WORDS: &str = "Write a message that is sent when an answer contains the text you name. The chat composer changes form: the text to search for goes in a field of its own, and the message in the box. The message is sent ahead of the messages that were only queued, and only when an answer contains that text.";

// A message that was sent with the word of the programmed message before it. The text it waits
// for are given in the chat composer and not among the text of the message, so a message that
// came this way was written out manually and cannot be answered from the text alone.
pub fn asked_as_program(text: &str) -> bool {
    let trimmed = text.trim_start();
    let Some(rest) = trimmed.strip_prefix(PROGRAM_WORD) else {
        return false;
    };
    // `/programme` is no command: the word has to be on its own.
    match rest.chars().next() {
        None => true,
        Some(next) => next.is_whitespace(),
    }
}

// The command a message begins with, and the text left when it is removed. Only the first word
// of a message is a command, and only when it begins with the marker of one. A message that is
// only the command returns with nothing after it, which is the user asking for the mode without
// a request in it.
pub fn split(text: &str) -> (Option<Command>, String) {
    let trimmed = text.trim_start();
    for (word, command) in [
        (COMMAND_WORD, Command::Plan),
        (crate::goal::COMMAND_WORD, Command::Goal),
    ] {
        let Some(rest) = trimmed.strip_prefix(word) else {
            continue;
        };
        // The command has to stand as a whole word: `/planet` and `/goalkeeper` are text.
        if rest.chars().next().is_some_and(|next| !next.is_whitespace()) {
            continue;
        }
        return (Some(command), rest.trim_start().to_string());
    }
    (None, text.to_string())
}

// The commands of the chat composer, in the order they are listed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Plan,
    Goal,
}

impl Command {
    // The word the user writes for this command, marker included.
    pub fn word(&self) -> &'static str {
        match self {
            Command::Plan => COMMAND_WORD,
            Command::Goal => crate::goal::COMMAND_WORD,
        }
    }

    // What the command is for, for the list the chat composer puts up.
    pub fn words(&self) -> &'static str {
        match self {
            Command::Plan => COMMAND_WORDS,
            Command::Goal => crate::goal::COMMAND_WORDS,
        }
    }
}

// The text of the command list, for the state: the word of the command and what it is for.
pub fn list() -> Vec<Value> {
    vec![
        json!({
            "name": Command::Plan.word(),
            "words": Command::Plan.words(),
        }),
        json!({
            "name": Command::Goal.word(),
            "words": Command::Goal.words(),
        }),
        json!({
            "name": PROGRAM_WORD,
            "words": PROGRAM_WORDS,
        }),
    ]
}

// Change the plan of a chat. The plan is read whole, every change is matched against it as it
// is now, and all of the plan is written again or not at all. A call that names no change, or
// too many of them, is refused with a word about what the tool takes.
pub fn edit_plan(name: &str, raw: &str) -> tools::Report {
    if chat_of(name).is_none() {
        return tools::Report {
            ok: false,
            output: String::from("This chat has no plan, so there is \
        nothing to change."),
        };
    }
    let changes = match changes_of(raw) {
        Ok(made) => made,
        Err(why) => return tools::Report { ok: false, output: why },
    };
    let before: String = read(name).unwrap_or_default();
    // The changes go one after another, each against the plan as the change before it left it.
    // A plan is written in parts, and a model that gives a part and a change in one call means
    // the one to follow the other.
    let mut after = before;
    let mut made = 0usize;
    let mut added = 0usize;
    for change in &changes {
        if change.old.is_empty() {
            // The marker of an empty place: the text go at the end of the plan, which is how a
            // plan of many parts is written.
            if !after.is_empty() && !after.ends_with('\n') {
                after.push('\n');
            }
            after.push_str(&change.new);
            added += 1;
            made += 1;
            continue;
        }
        let places = count_of(&after, &change.old);
        if places == 0 {
            return tools::Report {
                ok: false,
                output: format!(
                    "Nothing was written. This text is not in the plan: {}{}",
                    quote_head(&change.old),
                    if after.trim().is_empty() {
                        String::from(
                            " (the plan holds nothing yet: write the whole plan with `init_plan`, or put a whole part in new_string with an empty old_string)",
                        )
                    } else {
                        String::new()
                    }
                ),
            };
        }
        if places > 1 && !change.all {
            return tools::Report {
                ok: false,
                output: format!(
                    "Nothing was written. This text appears {} places in \
        the plan, and that is not enough to say which one was meant: {}",
                    places,
                    quote_head(&change.old)
                ),
            };
        }
        if change.all {
            after = after.replace(change.old.as_str(), change.new.as_str());
            made += places;
        } else {
            let found = after.find(change.old.as_str()).unwrap_or(0);
            after.replace_range(found..found + change.old.len(), &change.new);
            made += 1;
        }
    }
    if after.len() > GREATEST_PLAN_BYTES {
        return tools::Report {
            ok: false,
            output: String::from(
                "Nothing was written. The plan would grow past the \
        greatest length this program stores.",
            ),
        };
    }
    if let Err(why) = write(name, &after) {
        return tools::Report {
            ok: false,
            output: format!("The plan could not be written: {}", why),
        };
    }
    let mut said = format!(
        "{} in the plan. The plan now holds {} lines.",
        if added > 0 && made == added {
            format!("{} part(s) added", added)
        } else {
            format!("{} change(s) made", made)
        },
        lines_of(&after)
    );
    if let Some(head) = head_of(&after) {
        said.push_str("\nThe plan begins:\n");
        said.push_str(&head);
    }
    tools::Report { ok: true, output: said }
}

// One change of the plan.
struct Change {
    old: String,
    new: String,
    all: bool,
}

// The changes of one call. The call may be given whole, or in parts that are still arriving,
// and a call whose text is cut in half is answered with the text that record it rather than
// with a half change to the plan.
fn changes_of(raw: &str) -> Result<Vec<Change>, String> {
    let value: Value = serde_json::from_str(raw).map_err(|_| {
        String::from("The call was not valid JSON, so nothing \
                was written.")
    })?;
    let Some(items) = value.get("edits").and_then(|made| made.as_array()) else {
        return Err(String::from(
            "The call named no changes. Give the changes of the plan in `edits`.",
        ));
    };
    if items.is_empty() {
        return Err(String::from("The call named no changes."));
    }
    if items.len() > MAX_EDITS {
        return Err(format!(
            "The call requests {} changes, and this tool takes at most {}.",
            items.len(),
            MAX_EDITS
        ));
    }
    let mut out: Vec<Change> = Vec::with_capacity(items.len());
    for item in items {
        let old = item
            .get("old_string")
            .and_then(|one| one.as_str())
            .ok_or_else(|| String::from("A change of the call has no old_string."))?
            .to_string();
        let new = item
            .get("new_string")
            .and_then(|one| one.as_str())
            .ok_or_else(|| String::from("A change of the call has no new_string."))?
            .to_string();
        let all = item
            .get("replace_all")
            .and_then(|one| one.as_bool())
            .unwrap_or(false);
        out.push(Change { old, new, all });
    }
    Ok(out)
}

// How many places one run of text is in another.
fn count_of(text: &str, needle: &str) -> usize {
    if needle.is_empty() {
        return 0;
    }
    let mut places = 0usize;
    let mut from = 0usize;
    while let Some(found) = text[from..].find(needle) {
        places += 1;
        from += found + needle.len();
    }
    places
}

// The first lines of the plan, for the result. The model is given a look at where it is, not a
// copy of the file.
fn head_of(words: &str) -> Option<String> {
    let mut out = String::new();
    let mut lines = 0usize;
    for line in words.lines() {
        if line.trim().is_empty() {
            continue;
        }
        out.push_str(line.trim_end());
        out.push('\n');
        lines += 1;
        if lines >= 12 || out.len() > 1200 {
            break;
        }
    }
    if out.is_empty() {
        return None;
    }
    Some(out)
}

// How many lines a run of text holds.
fn lines_of(words: &str) -> usize {
    if words.is_empty() {
        return 0;
    }
    words.lines().count()
}

// A run of text for an error word, truncated and on one line.
fn quote_head(words: &str) -> String {
    let one_line: String = words
        .chars()
        .map(|one| if one == '\n' || one == '\r' { ' ' } else { one })
        .collect();
    let cut: String = one_line.chars().take(80).collect();
    format!("`{}`", cut.trim())
}

// The name of a chat id turned into what a file name may hold. The id of a chat is already
// hexadecimal; this is the guard for a name that came from elsewhere.
fn neat(name: &str) -> String {
    name.chars()
        .filter(|one| one.is_ascii_alphanumeric())
        .take(40)
        .collect()
}

// ------------------------------------------------ questions of the plan
//
// A model that plans needs items settled that only the user can settle. The questionnaire is
// the way to request them: any number of questions at once, each with any number of answers to
// choose from, the user free to choose several of them, and nothing sent until the user presses
// the send marker. No clock runs on a questionnaire, for the plan is not served on time.

pub const ASK_NAME: &str = "ask_plan";

// The most questions one questionnaire may hold, and the most answers one question may propose.
// Both are kept generous, for a plan of a large piece of work has a large number of items to
// settle.
const GREATEST_QUESTIONS: usize = 12;
const GREATEST_CHOICES: usize = 12;

// The call the model is proposed.
pub fn ask_definition() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": ASK_NAME,
            "description": "Ask only when work cannot proceed without a decision from the user. Ask up to 12 questions with up to 12 choices each; set many for multiple selections or leave choices empty for a written answer. There is no timeout. In planning mode use this instead of ask_user, and keep questions and unresolved choices out of the plan.",
            "parameters": {
                "type": "object",
                "properties": {
                    "questions": {
                        "type": "array",
                        "description": "Questions in display order (up to 12).",
                        "items": {
                            "type": "object",
                            "properties": {
                                "question": {
                                    "type": "string",
                                    "description": "One concise, self-contained question.",
                                },
                                "choices": {
                                    "type": "array",
                                    "description": "Suggested answers; empty lets the user write one.",
                                    "items": { "type": "string" }
                                },
                                "many": {
                                    "type": "boolean",
                                    "description": "Allow multiple selections; defaults to false."
                                }
                            },
                            "required": ["question"]
                        }
                    }
                },
                "required": ["questions"]
            }
        }
    })
}

// The questions of one call, read and prepared. A question with no text in it is an error in
// the call, for a questionnaire the user cannot read is no help to the plan.
pub fn questions_of(raw: &str) -> Result<Vec<crate::engine::AskItem>, String> {
    let Ok(call) = serde_json::from_str::<Value>(raw) else {
        return Err(String::from(
            "The call was not valid JSON, so nothing was asked.",
        ));
    };
    let Some(items) = call.get("questions").and_then(|made| made.as_array()) else {
        return Err(String::from(
            "The call named no questions. Give the questions of the plan in `questions`.",
        ));
    };
    if items.is_empty() {
        return Err(String::from("The call named no questions."));
    }
    if items.len() > GREATEST_QUESTIONS {
        return Err(format!(
            "{} questions in one questionnaire is over the limit of {}. Ask the most important of them now, and the rest in a call of their own.",
            items.len(),
            GREATEST_QUESTIONS
        ));
    }
    let mut out = Vec::new();
    for (number, item) in items.iter().enumerate() {
        let index = number + 1;
        let prompt = item
            .get("question")
            .and_then(|made| made.as_str())
            .map(|made| made.trim().to_string())
            .filter(|made| !made.is_empty())
            .ok_or_else(|| format!("question {} has no words in it", index))?;
        let mut choices: Vec<String> = Vec::new();
        if let Some(list) = item.get("choices").and_then(|made| made.as_array()) {
            for given in list.iter().take(GREATEST_CHOICES) {
                if let Some(word) = given.as_str() {
                    let word = word.trim();
                    if !word.is_empty() {
                        choices.push(word.to_string());
                    }
                }
            }
        }
        out.push(crate::engine::AskItem {
            prompt,
            choices,
            many: item
                .get("many")
                .and_then(|made| made.as_bool())
                .unwrap_or(false),
        });
    }
    Ok(out)
}

// ---------------------------------------------------- reading the plan back
//
// The text of the plan, exactly as they are now in the file. A model that changed the plan
// needs to see how the change landed, so the plan is read back to it line by line, as the
// search command of a terminal reads a file back: the number each line is on, the lines about
// each place that was found, and the start or the end of the file when those were what was
// requested.

pub const READ_NAME: &str = "read_plan";

// The call the model is proposed. It takes no file: the plan of this chat is the only text it
// can read, and it is never changed by reading it.
pub fn read_definition() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": READ_NAME,
            "description": "Read this plan with numbered lines. Search with pattern and optional before/after context and at_most; use head, tail or from_line/to_line to read a range. With no filters, returns the whole plan. This tool changes nothing.",
            "parameters": {
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "Text to search for; omit to read the whole plan or a selected range."
                    },
                    "ignore_case": {
                        "type": "boolean",
                        "description": "Ignore letter case; defaults to false."
                    },
                    "before": {
                        "type": "number",
                        "description": "Context lines before each match."
                    },
                    "after": {
                        "type": "number",
                        "description": "Context lines after each match."
                    },
                    "at_most": {
                        "type": "number",
                        "description": "Maximum matches to return; all by default."
                    },
                    "head": {
                        "type": "number",
                        "description": "Read the first N lines."
                    },
                    "tail": {
                        "type": "number",
                        "description": "Read the last N lines."
                    },
                    "from_line": {
                        "type": "number",
                        "description": "First line to read (1-based)."
                    },
                    "to_line": {
                        "type": "number",
                        "description": "Last line to read, inclusive."
                    }
                },
                "required": []
            }
        }
    })
}

// A count out of the call, when one was given. A number written between quotes is read as a
// count too, for a model writes counts that way as often as not.
fn asked(call: &Value, key: &str) -> Option<usize> {
    let given = call.get(key)?;
    if let Some(count) = given.as_u64() {
        return Some(count as usize);
    }
    if let Some(count) = given.as_i64() {
        return if count < 0 { None } else { Some(count as usize) };
    }
    given.as_str().and_then(|word| word.trim().parse::<usize>().ok())
}

// The lines of the answer, each with the number it is on in the plan. The places given in
// `groups` begin a new group, and a group is set apart from the one before it by a line of two
// markers, as the search command of a terminal does. The number of the line is carried with it,
// so a read of a part of the plan answers with the numbers of the file and not with the count
// of the answer.
fn numbered(held: &[(usize, &str)], groups: &[usize]) -> String {
    let mut out = String::new();
    for (number, (place, line)) in held.iter().enumerate() {
        if groups.contains(&number) && number > 0 {
            out.push_str("--\n");
        }
        out.push_str(&format!("{}: {}\n", place + 1, line));
    }
    out
}

// Read the plan back. The plan is checked line by line, so every number the answer carries is
// the number of the file. The two ways of reading it - a search, and a read of a part - fill in
// the text of the answer itself, which is why the text is named before either of them has
// run.
#[allow(clippy::needless_late_init)]
pub fn read_plan(name: &str, raw: &str) -> tools::Report {
    let failed = |output: String| tools::Report { ok: false, output };
    if chat_of(name).is_none() {
        return failed(String::from(
            "This chat has no plan, so there is nothing to read back.",
        ));
    }
    let Some(words) = read(name) else {
        return failed(String::from(
            "The plan could not be read, so nothing was answered.",
        ));
    };
    // A new plan holds nothing at all, and there is no line to read in it. The word about it
    // names the tool that writes the plan whole.
    if words.trim().is_empty() {
        return tools::Report {
            ok: true,
            output: String::from(
                "The plan holds nothing yet. Write the whole plan with `init_plan`.",
            ),
        };
    }
    let Ok(call) = serde_json::from_str::<Value>(raw) else {
        return failed(String::from(
            "The call was not valid JSON, so nothing was answered.",
        ));
    };
    // The plan is checked line by line. A marker of a new line at the very end closes the last
    // line rather than opening one, so it is not a line of the plan, as it is not in a terminal
    // either.
    let mut lines: Vec<&str> = words.split('\n').collect();
    if words.ends_with('\n') {
        lines.pop();
    }
    let pattern = call.get("pattern").and_then(|v| v.as_str()).unwrap_or("");
    // The lines of the answer, and the places among them where a new group begins. The two are
    // worked out below, one way for a search and one for a read of a part. The lines of the
    // answer, with the number each of them is on in the plan, and the places among them where a
    // new group begins.
    let mut held: Vec<(usize, &str)> = Vec::new();
    let mut groups: Vec<usize> = Vec::new();
    let said: String;
    if !pattern.is_empty() {
        let ignoring = call
            .get("ignore_case")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let needle = if ignoring { pattern.to_lowercase() } else { pattern.to_string() };
        let before = asked(&call, "before").unwrap_or(0);
        let after = asked(&call, "after").unwrap_or(0);
        let ceiling = asked(&call, "at_most").unwrap_or(usize::MAX);
        let mut found = 0usize;
        // One past the last line the answer holds already, so a place that falls among lines
        // given is joined to them instead of being given twice.
        let mut held_to = 0usize;
        for (number, line) in lines.iter().enumerate() {
            let here = if ignoring { line.to_lowercase() } else { line.to_string() };
            if !here.contains(needle.as_str()) {
                continue;
            }
            found += 1;
            let from = number.saturating_sub(before);
            let to = (number + after + 1).min(lines.len());
            if from > held_to && !held.is_empty() {
                groups.push(held.len());
            }
            let first = from.max(held_to);
            for (offset, line) in lines[first..to].iter().enumerate() {
                held.push((first + offset, line));
            }
            held_to = held_to.max(to);
            if found >= ceiling {
                break;
            }
        }
        if found == 0 {
            return tools::Report {
                ok: true,
                output: format!(
                    "Nothing was found. \"{}\" is not in the plan, which holds {} lines.",
                    pattern,
                    lines.len()
                ),
            };
        }
        said = format!(
            "{} place{} of \"{}\", in a plan of {} lines.\n",
            found,
            if found == 1 { "" } else { "s" },
            pattern,
            lines.len()
        );
    } else {
        // Nothing to search for: the start of the plan, its end, a range of its lines, or all
        // of it.
        let head = asked(&call, "head");
        let tail = asked(&call, "tail");
        let from = asked(&call, "from_line").map(|one| one.saturating_sub(1).min(lines.len()));
        let to = asked(&call, "to_line").map(|one| one.min(lines.len()));
        let mut parts: Vec<(usize, usize)> = Vec::new();
        match (from, to) {
            (Some(first), Some(last)) if last > first => parts.push((first, last)),
            (Some(first), None) => parts.push((first, lines.len())),
            (None, Some(last)) => parts.push((0, last)),
            _ => {
                if let Some(count) = head {
                    parts.push((0, count.min(lines.len())));
                }
                if let Some(count) = tail {
                    parts.push((lines.len().saturating_sub(count), lines.len()));
                }
            }
        }
        if parts.is_empty() {
            parts.push((0, lines.len()));
        }
        let first = parts.first().map(|one| one.0).unwrap_or(0);
        let last = parts.last().map(|one| one.1).unwrap_or(0);
        for (number, part) in parts.iter().enumerate() {
            if number > 0 && part.0 < held.len() {
                continue;
            }
            if number > 0 {
                groups.push(held.len());
            }
            let first = part.0.max(if number > 0 { held.last().map(|one| one.0 + 1).unwrap_or(0) } else { 0 });
            for (offset, line) in lines[first..part.1].iter().enumerate() {
                held.push((first + offset, line));
            }
        }
        said = format!(
            "Lines {} to {} of a plan of {} lines.\n",
            first + 1,
            last,
            lines.len()
        );
    }
    let body = numbered(&held, &groups);
    tools::Report {
        ok: true,
        output: format!("{}{}", said, body),
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    // A plan is a row of the table of the plans, so a check that needs a plan of its own text
    // lays one down through the same door the tools write through, and takes the row away again
    // when the check is done.
    fn lay_down(chat: &str, text: &str) -> String {
        let name = file_name(chat);
        write(&name, text).expect("the plan is laid down");
        name
    }

    // The plan of a chat is in the database of the conversations, and only a name this module
    // wrote is a name it knows. A chat that goes into planning mode twice keeps the plan it
    // wrote.
    #[test]
    fn a_plan_of_a_chat_is_one_row_of_the_table() {
        let _guard = config::test_turn();
        let name = create("ab12cd34ef");
        assert_eq!(name, "ab12cd34ef.md");
        assert_eq!(chat_of(&name).as_deref(), Some("ab12cd34ef"));
        assert!(stands(&name), "planning mode makes the row of the plan");
        let held = words(&name).expect("the plan reads back");
        // A new plan holds nothing. Text of the program in a new plan were text the model had
        // to scan out of the plan again, and text the user read in the window of a plan that
        // was never written.
        assert!(held.trim().is_empty(), "{}", held);
        // A second look leaves the plan as it is now.
        lay_down("ab12cd34ef", "the plan as the model wrote it");
        create("ab12cd34ef");
        assert_eq!(words(&name).unwrap(), "the plan as the model wrote it");
        drop_plan("ab12cd34ef");
        assert!(!stands(&name), "the row goes with the chat");
        assert!(words(&name).is_none());
        // A name this module could not have written reaches no row at all.
        assert!(chat_of("").is_none());
        assert!(chat_of("../escape.md").is_none());
        assert!(chat_of("notes.md").is_none());
        assert!(!stands("notes.md"));
    }

    // The tool changes the plan by matching text, and an empty place adds a part at the end of
    // it.
    #[test]
    fn the_plan_is_changed_by_matching_words() {
        let _guard = config::test_turn();
        let name = create("1234567890");
        let first = write_init(
            &name,
            "{\"plan\":\"# The plan\\n\\nThe work is not written yet.\\n\"}",
        );
        assert!(first.ok, "{}", first.output);
        let made = edit_plan(
            &name,
            r#"{"edits":[{"old_string":"The work is not written yet.","new_string":"The work is written below."}]}"#,
        );
        assert!(made.ok, "{}", made.output);
        assert!(made.output.contains("1 change"), "{}", made.output);
        assert!(words(&name).unwrap().contains("The work is written below"));
        let added = edit_plan(
            &name,
            r#"{"edits":[{"old_string":"","new_string":"\n## Step one\n"}]}"#,
        );
        assert!(added.ok, "{}", added.output);
        let held = words(&name).unwrap();
        assert!(held.ends_with("## Step one\n"), "{}", held);
        drop_plan("1234567890");
    }

    // The first writing of a plan is one whole call. What was there before is written over, and
    // a call that carries no plan is refused, so a call that meant nothing never takes a
    // written plan away.
    #[test]
    fn the_first_writing_of_a_plan_is_one_call() {
        let _guard = config::test_turn();
        let name = create("8800880088");
        assert!(words(&name).unwrap().trim().is_empty(), "a new plan holds nothing");
        let begun = write_init(&name, r##"{"plan":"# The way in\n\nStep one.\n"}"##);
        assert!(begun.ok, "{}", begun.output);
        assert!(begun.output.contains("3 lines"), "{}", begun.output);
        assert!(begun.output.contains("# The way in"), "{}", begun.output);
        // Nothing was before, so nothing is reported about what was lost.
        assert!(!begun.output.contains("gone"), "{}", begun.output);
        assert_eq!(words(&name).unwrap(), "# The way in\n\nStep one.\n");
        // The plan may be removed through again in parts, for the tool that matches text has
        // text to match now.
        let part = edit_plan(
            &name,
            r#"{"edits":[{"old_string":"Step one.","new_string":"Step one. Then step two."}]}"#,
        );
        assert!(part.ok, "{}", part.output);
        // A second whole writing takes the plan that was with it, and records it.
        let again = write_init(&name, r##"{"plan":"# Another way in\n"}"##);
        assert!(again.ok, "{}", again.output);
        assert!(again.output.contains("replaced"), "{}", again.output);
        assert_eq!(words(&name).unwrap(), "# Another way in\n");
        // A call of no text, or of spaces only, writes nothing at all.
        let empty = write_init(&name, r#"{"plan":"   \n\n  "}"#);
        assert!(!empty.ok);
        assert!(empty.output.contains("no plan"), "{}", empty.output);
        assert_eq!(words(&name).unwrap(), "# Another way in\n");
        let missing = write_init(&name, r#"{"other":"words of a plan"}"#);
        assert!(!missing.ok);
        assert!(missing.output.contains("`plan`"), "{}", missing.output);
        let broken = write_init(&name, r#"{"plan":"cut off"#);
        assert!(!broken.ok);
        assert!(broken.output.contains("JSON"), "{}", broken.output);
        // A name that is not one of this module's own reaches no file at all.
        let stranger = write_init("notes.md", r#"{"plan":"out of the folder"}"#);
        assert!(!stranger.ok);
        drop_plan("8800880088");
    }

    // A change requested of a plan that holds nothing reports which tool writes the plan whole,
    // for there is nothing there to match against.
    #[test]
    fn an_empty_plan_says_which_tool_writes_it() {
        let _guard = config::test_turn();
        let name = create("7700770077");
        let made = edit_plan(
            &name,
            r#"{"edits":[{"old_string":"Step one","new_string":"Step two"}]}"#,
        );
        assert!(!made.ok);
        assert!(made.output.contains(INIT_NAME), "{}", made.output);
        // A part written into an empty plan with no place to match still remains.
        let part = edit_plan(&name, r##"{"edits":[{"old_string":"","new_string":"# Step one\n"}]}"##);
        assert!(part.ok, "{}", part.output);
        assert_eq!(words(&name).unwrap(), "# Step one\n");
        drop_plan("7700770077");
    }

    // A change that matches no place, or more than one, writes nothing at all.
    #[test]
    fn a_change_that_could_be_two_places_writes_nothing() {
        let _guard = config::test_turn();
        let name = lay_down("aabbccddeeff", "step and step again");
        let none = edit_plan(&name, r#"{"edits":[{"old_string":"nowhere","new_string":"x"}]}"#);
        assert!(!none.ok);
        assert!(none.output.contains("not in the plan"), "{}", none.output);
        let twice = edit_plan(&name, r#"{"edits":[{"old_string":"step","new_string":"part"}]}"#);
        assert!(!twice.ok);
        assert!(twice.output.contains("2 places"), "{}", twice.output);
        assert_eq!(words(&name).unwrap(), "step and step again");
        let all = edit_plan(
            &name,
            r#"{"edits":[{"old_string":"step","new_string":"part","replace_all":true}]}"#,
        );
        assert!(all.ok, "{}", all.output);
        assert_eq!(words(&name).unwrap(), "part and part again");
        drop_plan("aabbccddeeff");
    }

    // The word of the user, and the text left when the command is removed.
    #[test]
    fn the_command_stands_at_the_head_of_the_message() {
        let (command, rest) = split("/plan build the side bar");
        assert_eq!(command, Some(Command::Plan));
        assert_eq!(rest, "build the side bar");
        let (alone, nothing) = split("/plan");
        assert_eq!(alone, Some(Command::Plan));
        assert!(nothing.is_empty());
        let (none, kept) = split("please /plan later");
        assert_eq!(none, None);
        assert_eq!(kept, "please /plan later");
        let (planet, words) = split("/planets are far");
        assert_eq!(planet, None);
        assert_eq!(words, "/planets are far");
        let (goal, objective) = split("/goal fix and verify the parser");
        assert_eq!(goal, Some(Command::Goal));
        assert_eq!(objective, "fix and verify the parser");
        let (goal_alone, empty) = split("/goal");
        assert_eq!(goal_alone, Some(Command::Goal));
        assert!(empty.is_empty());
        let (not_goal, words) = split("/goalkeeper is a word");
        assert_eq!(not_goal, None);
        assert_eq!(words, "/goalkeeper is a word");
    }

    // The three tools name themselves, and none of them takes a place.
    #[test]
    fn the_tools_of_planning_carry_no_place() {
        let begun = init_definition();
        assert_eq!(begun["function"]["name"], json!(INIT_NAME));
        let wants = begun["function"]["parameters"]["required"]
            .as_array()
            .expect("a list of what the call wants");
        assert_eq!(wants.len(), 1, "the call wants `plan` and nothing else");
        assert_eq!(wants[0], json!("plan"));
        let first = begun["function"]["description"].as_str().unwrap();
        assert!(first.contains("no path"), "{}", first);
        assert!(
            first.contains("first writing"),
            "the tool must say that it means the first writing only: {}",
            first
        );
        let edit = edit_definition();
        assert_eq!(edit["function"]["name"], json!(EDIT_NAME));
        let wanted = edit["function"]["parameters"]["required"]
            .as_array()
            .expect("a list of what the call wants");
        assert_eq!(wanted.len(), 1, "the call wants `edits` and nothing else");
        assert_eq!(wanted[0], json!("edits"));
        let said = edit["function"]["description"].as_str().unwrap();
        assert!(said.contains("no path"), "{}", said);
        let ended = end_definition();
        assert_eq!(ended["function"]["name"], json!(END_NAME));
        assert!(ended["function"]["description"]
            .as_str()
            .unwrap()
            .contains("the user has stated"));
    }

    // The prompt reports what the model must do with the plan, and how detailed it must be.
    #[test]
    fn the_prompt_names_the_one_way_into_the_plan() {
        let _guard = config::test_turn();
        let said = system_prompt("ab12cd34ef.md");
        assert!(said.contains("PLANNING MODE IS ON"));
        assert!(said.contains("ab12cd34ef.md"));
        assert!(said.contains(INIT_NAME));
        assert!(said.contains(EDIT_NAME));
        assert!(said.contains("self-contained plan"));
        assert!(said.contains(END_NAME));
        // The plan is named, and no other file is proposed to it.
        assert!(!said.contains("edit_file"), "{}", said);
    }

    // The first rule of the mode, in the text of the prompt: nothing of the work is done while
    // planning mode remains. The ban is on writing and on every command that leaves an item
    // changed, and it is named by the types of command a model reaches for; the commands that
    // only look about stay open, so that the plan is still built on what the work is and not on
    // a guess.
    #[test]
    fn the_prompt_forbids_the_work_and_keeps_the_reading() {
        let _guard = config::test_turn();
        let said = system_prompt("ab12ab12ab.md");
        assert!(said.contains("You design the work; you do not execute it"), "{said}");
        assert!(said.contains("read-only tools"), "{said}");
        assert!(said.contains("do not change work files"), "{said}");
        assert!(said.contains("version control"), "{said}");
        assert!(said.contains("The only writable record is this chat's plan"), "{said}");
        assert!(!said.contains("edit_file"), "{said}");
        assert!(!said.contains("create_file"), "{said}");
    }

    // The second rule of the mode: `end_planning` is the only way out of planning mode, so a
    // model may not say the work has begun, and may not do a step of it in place of the call.
    // The text of the tool reports the same, for a model reads the tool list beside the prompt.
    #[test]
    fn the_prompt_names_the_one_way_out_of_planning_mode() {
        let _guard = config::test_turn();
        let said = system_prompt("cd34cd34cd.md");
        assert!(said.contains(&format!("Planning ends only through {END_NAME}")), "{said}");
        assert!(said.contains("only after the user explicitly approves"), "{said}");
        assert!(said.contains("A finished plan is not approval"), "{said}");
        let words = end_definition()["function"]["description"]
            .as_str()
            .expect("words")
            .to_string();
        assert!(
            words.contains("the only way planning mode ends"),
            "{words}"
        );
    }

    // A plan that was never written reads back as nothing written, and reports which tool
    // writes it whole: the model is never left with a blank space for a file.
    #[test]
    fn a_plan_that_was_never_written_reads_back_as_nothing() {
        let _guard = config::test_turn();
        let name = create("6600660066");
        let whole = read_plan(&name, "{}");
        assert!(whole.ok, "{}", whole.output);
        assert!(whole.output.contains("holds nothing"), "{}", whole.output);
        assert!(whole.output.contains(INIT_NAME), "{}", whole.output);
        let asked = read_plan(&name, r#"{"pattern":"step"}"#);
        assert!(asked.ok, "{}", asked.output);
        assert!(asked.output.contains("holds nothing"), "{}", asked.output);
        drop_plan("6600660066");
    }

    // The plan is read back with the lines about each place that was found, and two places that
    // do not touch are kept apart by a line of two markers. The numbers the answer carries are
    // the numbers of the file, so a read of a part of the plan reports where each line is in
    // it.
    #[test]
    fn the_plan_is_read_back_about_a_place() {
        let _guard = config::test_turn();
        let name = lay_down("7001001", "alpha step one\nbeta\ngamma step one\n");
        let made = read_plan(&name, r#"{"pattern":"step one","after":1}"#);
        assert!(made.ok, "{}", made.output);
        assert!(made.output.contains("2 places"), "{}", made.output);
        assert!(
            made.output.contains("1: alpha step one\n2: beta\n3: gamma step one\n"),
            "{}",
            made.output
        );
        // The two places touch, for the line after the first holds the line before the second,
        // so no marker is between them. With no line about the places they do not touch, and a
        // line of two markers does remain.
        let tight = read_plan(&name, r#"{"pattern":"step one"}"#);
        assert!(tight.ok, "{}", tight.output);
        assert!(
            tight.output.contains("1: alpha step one\n--\n3: gamma step one\n"),
            "{}",
            tight.output
        );
        // A count of places stops the search at the first of them.
        let one = read_plan(&name, r#"{"pattern":"step one","at_most":1}"#);
        assert!(one.output.contains("1 place"), "{}", one.output);
        assert!(!one.output.contains("gamma"), "{}", one.output);
        // The letters of the text is looked past when it is requested.
        let loose = read_plan(&name, r#"{"pattern":"ALPHA STEP ONE","ignore_case":true}"#);
        assert!(loose.output.contains("1 place"), "{}", loose.output);
        drop_plan("7001001");
    }

    // The start of the plan, a range of its lines, and all of it are read back with the numbers
    // of the file in front of the lines.
    #[test]
    fn a_part_of_the_plan_is_read_back() {
        let _guard = config::test_turn();
        let name = lay_down("7001002", "one\ntwo\nthree\nfour\n");
        let head = read_plan(&name, r#"{"head":2}"#);
        assert!(head.ok, "{}", head.output);
        assert!(head.output.contains("1: one\n2: two\n"), "{}", head.output);
        // A range of lines is requested by the numbers of the file, and answers with those
        // numbers and not with a count of its own.
        let middle = read_plan(&name, r#"{"from_line":2,"to_line":3}"#);
        assert!(middle.ok, "{}", middle.output);
        assert!(middle.output.contains("2: two\n3: three\n"), "{}", middle.output);
        assert!(!middle.output.contains("one"), "{}", middle.output);
        // The last lines of the plan, and all of it.
        let last = read_plan(&name, r#"{"tail":1}"#);
        assert!(last.output.contains("4: four"), "{}", last.output);
        let whole = read_plan(&name, "{}");
        assert!(whole.ok, "{}", whole.output);
        assert!(
            whole.output.contains("1: one\n2: two\n3: three\n4: four\n"),
            "{}",
            whole.output
        );
        drop_plan("7001002");
    }

    // A read of a long plan is not cut. It was kept to four hundred lines and twelve thousand
    // characters, with a line at the end telling the model to ask again for a part, so a model
    // that read a long plan through was given the start of it and had to ask again and again -
    // and a model that did not ask again wrote from a plan it believed it had read whole. The
    // whole of what was asked for comes back now; the size of the conversation is dealt with by
    // the handoff summary.
    #[test]
    fn a_long_plan_is_read_back_whole() {
        let _guard = config::test_turn();
        let long: String = (1..=900)
            .map(|number| format!("step {} of the plan, with a line of its own\n", number))
            .collect();
        let name = lay_down("7001004", &long);
        let whole = read_plan(&name, "{}");
        assert!(whole.ok, "{}", whole.output);
        assert!(
            !whole.output.contains("the rest was left out"),
            "the read of the plan was cut after {} characters",
            whole.output.chars().count()
        );
        let lines: Vec<&str> = whole
            .output
            .lines()
            .filter(|one| one.chars().next().is_some_and(|c| c.is_ascii_digit()))
            .collect();
        assert_eq!(lines.len(), 900, "the read of the plan came back part of the way");
        assert!(
            whole.output.contains("900: step 900 of the plan"),
            "the end of the plan is not there"
        );
        // A read that asked for a range of lines is not cut either, however long the range is.
        let wide = read_plan(&name, r#"{"from_line":200,"to_line":799}"#);
        assert!(
            !wide.output.contains("the rest was left out"),
            "the read of a range was cut"
        );
        assert!(wide.output.contains("200: step 200 of the plan"), "{}", wide.output);
        assert!(wide.output.contains("799: step 799 of the plan"), "the range stopped short");
        drop_plan("7001004");
    }

    // A search that found nothing records it plainly, for a model that was answered with
    // silence might take it for a plan that holds the text.
    #[test]
    fn a_search_of_the_plan_that_found_nothing_says_so() {
        let _guard = config::test_turn();
        let name = lay_down("7001003", "alpha\n");
        let made = read_plan(&name, r#"{"pattern":"zebra step nine"}"#);
        assert!(made.ok, "{}", made.output);
        assert!(made.output.starts_with("Nothing was found"), "{}", made.output);
        drop_plan("7001003");
    }

    // The questions of the plan go to one tool. The prompt and the text of the tool both
    // reports that the questionnaire is the way to ask in planning mode, and not the card over
    // the chat composer, which is removed when its few seconds are spent, nor a question
    // written in the text of an answer.
    #[test]
    fn the_prompt_sends_the_questions_to_the_questionnaire() {
        let said = system_prompt("plan-7.md");
        assert!(said.contains(ASK_NAME), "{said}");
        assert!(said.contains("ask_user"), "{said}");
        assert!(said.contains("through ask_plan"), "{said}");
        let words = ask_definition()["function"]["description"]
            .as_str()
            .expect("words")
            .to_string();
        assert!(words.contains("instead of `ask_user`"), "{words}");
        assert!(words.contains("no timeout"), "{words}");
    }

    // The prompt reports the model that the user never sees the plan, names the questionnaire
    // and the read-back as the two ways of reaching the user and the plan, and forbids
    // questions written into the plan.
    #[test]
    fn the_prompt_keeps_questions_out_of_the_plan() {
        let said = system_prompt("plans/plan-1.md");
        assert!(said.contains("user cannot see the plan"), "{}", said);
        assert!(said.contains(ASK_NAME), "{}", said);
        assert!(said.contains(READ_NAME), "{}", said);
        assert!(said.contains("keep questions and unresolved choices out of the plan"), "{}", said);
        let definition = ask_definition();
        let ask = definition["function"]["description"]
            .as_str()
            .expect("questionnaire instructions");
        assert!(ask.contains("There is no timeout"), "{ask}");
    }

    // A questionnaire is any number of questions, each with the answers the user may choose
    // from and the marker of whether several may be chosen. A question with no text in it is
    // refused, for the user cannot read it.
    #[test]
    fn a_questionnaire_is_read_from_the_call() {
        let raw = r#"{"questions":[
            {"question":"Which theme first?","choices":["pale","dark","grey"],"many":true},
            {"question":"What is the name?","choices":[]}
        ]}"#;
        let items = questions_of(raw).expect("two questions");
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].choices.len(), 3);
        assert!(items[0].many);
        assert!(!items[1].many);
        assert!(items[1].choices.is_empty());
        let empty = questions_of(r#"{"questions":[{"question":"  "}]}"#);
        assert!(empty.is_err());
        let none = questions_of(r#"{"questions":[]}"#);
        assert!(none.is_err());
        let too_many = format!(
            "{{\"questions\":[{}]}}",
            (0..13)
                .map(|n| format!("{{\"question\":\"q {}\"}}", n))
                .collect::<Vec<String>>()
                .join(",")
        );
        assert!(questions_of(&too_many).is_err());
    }

    // The word of the programmed message is on its own, as the word of the plan does: a longer
    // word that begins the same way is no command, and neither is the word in the middle of a
    // sentence.
    #[test]
    fn the_word_of_the_programmed_message_stands_on_its_own() {
        assert!(asked_as_program("/program"));
        assert!(asked_as_program("  /program say it again"));
        assert!(!asked_as_program("/programme the work"));
        assert!(!asked_as_program("/plan"));
        assert!(!asked_as_program("read /program to me"));
    }
}
